use std::time::{Duration, Instant};

use aximo_core::{PartialSchedule, SessionError, ShortAudioRequest};
use aximo_inference::engine::{InferenceError, StreamingSpeechSession};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, mpsc::error::TrySendError, oneshot};

use crate::{
    app::AppState,
    inference_task::{run_observed_blocking_inference_with_timeout, BlockingInferenceError},
    ws::protocol::{ClientEvent, ServerEvent},
};

// 5 seconds of pcm_s16le 16 kHz mono audio.
const REALTIME_PARTIAL_WINDOW_BYTES: usize = 160_000;
const PCM_SAMPLE_RATE: u64 = 16_000;
const PCM_BYTES_PER_SAMPLE: usize = 2;
const WRITER_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);
const CLOSE_FRAME_LINGER: Duration = Duration::from_millis(50);

pub async fn realtime_socket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (event_tx, mut event_rx) =
        mpsc::channel::<ServerEvent>(state.realtime_event_channel_capacity);
    let (overflow_tx, mut overflow_rx) = mpsc::channel::<()>(1);
    let (close_tx, mut close_rx) = oneshot::channel::<()>();
    let mut writer = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut close_rx => {
                    let _ = sender.send(Message::Close(None)).await;
                    tokio::time::sleep(CLOSE_FRAME_LINGER).await;
                    break;
                }
                event = event_rx.recv() => {
                    let Some(event) = event else {
                        break;
                    };
                    let message = serde_json::to_string(&event).expect("serialize websocket event");
                    if sender.send(Message::Text(message.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut active_session: Option<ActiveRealtimeSession> = None;
    let mut shutdown = state.shutdown.subscribe();

    loop {
        macro_rules! queue_or_break {
            ($event:expr $(,)?) => {
                if !queue_event_or_overflow(&state, &event_tx, &overflow_tx, $event) {
                    break;
                }
            };
        }

        let message = tokio::select! {
            _ = shutdown.changed() => break,
            _ = overflow_rx.recv() => break,
            message = receiver.next() => message,
        };
        let Some(Ok(message)) = message else {
            break;
        };

        match message {
            Message::Text(text) => {
                let Ok(client_event) = serde_json::from_str::<ClientEvent>(&text) else {
                    queue_or_break!(ServerEvent::error(
                        "invalid_client_event",
                        "failed to parse client event"
                    ),);
                    continue;
                };

                match client_event.event.as_str() {
                    "start" => {
                        if active_session.is_some() {
                            queue_or_break!(ServerEvent::error(
                                "duplicate_start",
                                "session already started for this socket",
                            ),);
                            continue;
                        }

                        let health_admission = match admit_realtime_engine(&state) {
                            Ok(admission) => admission,
                            Err(reason) => {
                                queue_or_break!(ServerEvent::error("engine_degraded", reason),);
                                continue;
                            }
                        };

                        match state.scheduler.try_acquire_realtime_session() {
                            Ok(permit) => {
                                let session_id = state
                                    .session_manager
                                    .start_session(permit, state.realtime_session_limits);
                                let native_stream = if state
                                    .realtime_engine
                                    .capabilities()
                                    .supports_native_streaming
                                {
                                    match state.realtime_engine.start_streaming_session() {
                                        Ok(stream) => Some(stream),
                                        Err(error) => {
                                            let _ =
                                                state.session_manager.finish_session(&session_id);
                                            health_admission.cancel(&state);
                                            queue_or_break!(ServerEvent::error(
                                                "streaming_start_failed",
                                                error.to_string(),
                                            ),);
                                            continue;
                                        }
                                    }
                                } else {
                                    None
                                };

                                state.metrics.inc_ws_sessions_active();
                                active_session = Some(match native_stream {
                                    Some(stream) => ActiveRealtimeSession::Native {
                                        session_id: session_id.clone(),
                                        health_admission,
                                        stream,
                                        started_at: Instant::now(),
                                        bytes_received: 0,
                                    },
                                    None => ActiveRealtimeSession::Buffered {
                                        session_id: session_id.clone(),
                                        health_admission,
                                    },
                                });
                                queue_or_break!(ServerEvent::session_started(session_id));
                            }
                            Err(_) => {
                                health_admission.cancel(&state);
                                queue_or_break!(ServerEvent::error(
                                    "realtime_capacity_exhausted",
                                    "realtime session capacity exhausted",
                                ),);
                            }
                        }
                    }
                    "stop" => {
                        if let Some(session) = active_session.take() {
                            finish_active_session(&state, session, &event_tx, &overflow_tx).await;
                        } else {
                            queue_or_break!(ServerEvent::error(
                                "no_active_session",
                                "stop requested without an active session",
                            ),);
                        }
                    }
                    _ => {
                        queue_or_break!(ServerEvent::error(
                            "unsupported_client_event",
                            format!("unsupported client event: {}", client_event.event),
                        ),);
                    }
                }
            }
            Message::Binary(chunk) => {
                if let Some(session) = active_session.as_mut() {
                    if chunk.len() % PCM_BYTES_PER_SAMPLE != 0 {
                        queue_or_break!(ServerEvent::error(
                            "invalid_audio_chunk",
                            "pcm_s16le realtime chunks must be aligned to 16-bit samples",
                        ),);
                        continue;
                    }

                    match session {
                        ActiveRealtimeSession::Buffered { session_id, .. } => {
                            match state.session_manager.append_audio(session_id, &chunk) {
                                Ok(()) => {
                                    let schedule = state
                                        .session_manager
                                        .maybe_begin_partial(
                                            session_id,
                                            state.realtime_partial_limits,
                                        )
                                        .unwrap_or(PartialSchedule::Skip);

                                    if matches!(schedule, PartialSchedule::StartNow) {
                                        spawn_partial_inference(
                                            state.clone(),
                                            session_id.to_string(),
                                            event_tx.clone(),
                                            overflow_tx.clone(),
                                        );
                                    }
                                }
                                Err(SessionError::SessionTooLarge) => {
                                    cleanup_active_session(&state, &mut active_session);
                                    queue_or_break!(ServerEvent::error(
                                        "realtime_session_too_large",
                                        "realtime session exceeded configured byte limit",
                                    ),);
                                }
                                Err(SessionError::SessionTooLong) => {
                                    cleanup_active_session(&state, &mut active_session);
                                    queue_or_break!(ServerEvent::error(
                                        "realtime_session_too_long",
                                        "realtime session exceeded configured duration limit",
                                    ),);
                                }
                                Err(SessionError::MissingSession) => {
                                    queue_or_break!(ServerEvent::error(
                                        "audio_append_failed",
                                        "failed to append audio to the active realtime session",
                                    ),);
                                }
                            }
                        }
                        ActiveRealtimeSession::Native {
                            stream,
                            started_at,
                            bytes_received,
                            ..
                        } => {
                            if started_at.elapsed() > state.realtime_session_limits.max_duration {
                                cleanup_active_session(&state, &mut active_session);
                                queue_or_break!(ServerEvent::error(
                                    "realtime_session_too_long",
                                    "realtime session exceeded configured duration limit",
                                ),);
                                continue;
                            }
                            if bytes_received.saturating_add(chunk.len())
                                > state.realtime_session_limits.max_bytes
                            {
                                cleanup_active_session(&state, &mut active_session);
                                queue_or_break!(ServerEvent::error(
                                    "realtime_session_too_large",
                                    "realtime session exceeded configured byte limit",
                                ),);
                                continue;
                            }

                            *bytes_received = bytes_received.saturating_add(chunk.len());
                            let health_component =
                                format!("realtime_partial:{}", state.realtime_engine_name);
                            match stream.accept_pcm_chunk(&chunk) {
                                Ok(Some(result)) => {
                                    state.runtime_health.record_success(health_component);
                                    queue_or_break!(ServerEvent::partial_text(result.text));
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    record_native_inference_health(
                                        &state,
                                        &health_component,
                                        "realtime_partial",
                                        &error,
                                    );
                                    cleanup_active_session(&state, &mut active_session);
                                    queue_or_break!(ServerEvent::error(
                                        "inference_failed",
                                        error.to_string(),
                                    ),);
                                }
                            }
                        }
                    }
                } else {
                    queue_or_break!(ServerEvent::error(
                        "no_active_session",
                        "binary audio received before start",
                    ),);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    cleanup_active_session(&state, &mut active_session);
    let _ = close_tx.send(());
    drop(event_tx);
    tokio::select! {
        result = &mut writer => {
            let _ = result;
            return;
        }
        _ = tokio::time::sleep(WRITER_DRAIN_TIMEOUT) => writer.abort(),
    }
    let _ = writer.await;
}

fn spawn_partial_inference(
    state: AppState,
    session_id: String,
    event_tx: mpsc::Sender<ServerEvent>,
    overflow_tx: mpsc::Sender<()>,
) {
    tokio::spawn(async move {
        loop {
            let wait_started_at = Instant::now();
            let _inference_permit = state.scheduler.acquire_realtime_inference().await;
            let wait_elapsed = wait_started_at.elapsed();
            let audio_bytes = match state
                .session_manager
                .recent_audio_snapshot(&session_id, REALTIME_PARTIAL_WINDOW_BYTES)
            {
                Ok(audio_bytes) => audio_bytes,
                Err(SessionError::MissingSession) => {
                    state.metrics.record_realtime_stale_partial_skip();
                    break;
                }
                Err(SessionError::SessionTooLarge | SessionError::SessionTooLong) => break,
            };
            let audio_duration_ms = pcm_duration_ms(audio_bytes.len());
            let request = ShortAudioRequest {
                audio_bytes,
                content_type: "audio/pcm".to_string(),
                engine: None,
                language_hint: None,
                timestamps: false,
            };

            let inference_started_at = Instant::now();
            let health_component = format!("realtime_partial:{}", state.realtime_engine_name);
            let inference_result = run_observed_blocking_inference_with_timeout(
                state.realtime_engine.clone(),
                request,
                state.realtime_partial_timeout,
                state.metrics.clone(),
                "realtime_partial",
            )
            .await;
            let inference_elapsed = inference_started_at.elapsed();
            state.metrics.record_inference(
                "realtime_partial",
                wait_elapsed,
                inference_elapsed,
                audio_duration_ms,
            );
            let follow_up = match state.session_manager.complete_partial(&session_id) {
                Ok(follow_up) => follow_up,
                Err(SessionError::MissingSession) => {
                    state.metrics.record_realtime_stale_partial_skip();
                    break;
                }
                Err(SessionError::SessionTooLarge | SessionError::SessionTooLong) => break,
            };

            match inference_result {
                Ok(result) => {
                    state.runtime_health.record_success(health_component);
                    if !queue_event_or_overflow(
                        &state,
                        &event_tx,
                        &overflow_tx,
                        ServerEvent::partial_text(result.text),
                    ) {
                        let _ = overflow_tx.try_send(());
                        break;
                    }
                }
                Err(error) => {
                    record_inference_health(&state, &health_component, "realtime_partial", &error);
                    if !queue_event_or_overflow(
                        &state,
                        &event_tx,
                        &overflow_tx,
                        map_realtime_inference_error(error, "realtime partial inference timed out"),
                    ) {
                        let _ = overflow_tx.try_send(());
                        break;
                    }
                }
            }

            if !matches!(follow_up, PartialSchedule::StartNow) {
                break;
            }
            state.metrics.record_realtime_partial_coalesced();
        }
    });
}

enum ActiveRealtimeSession {
    Buffered {
        session_id: String,
        health_admission: RealtimeHealthAdmission,
    },
    Native {
        session_id: String,
        health_admission: RealtimeHealthAdmission,
        stream: Box<dyn StreamingSpeechSession>,
        started_at: Instant,
        bytes_received: usize,
    },
}

async fn finish_active_session(
    state: &AppState,
    session: ActiveRealtimeSession,
    event_tx: &mpsc::Sender<ServerEvent>,
    overflow_tx: &mpsc::Sender<()>,
) {
    match session {
        ActiveRealtimeSession::Buffered {
            session_id,
            health_admission,
        } => {
            let audio_bytes = state
                .session_manager
                .finish_session(&session_id)
                .unwrap_or_default();
            state.metrics.dec_ws_sessions_active();
            let audio_duration_ms = pcm_duration_ms(audio_bytes.len());
            let request = ShortAudioRequest {
                audio_bytes,
                content_type: "audio/pcm".to_string(),
                engine: None,
                language_hint: None,
                timestamps: false,
            };

            let wait_started_at = Instant::now();
            let _inference_permit = state.scheduler.acquire_realtime_inference().await;
            let wait_elapsed = wait_started_at.elapsed();

            let inference_started_at = Instant::now();
            let health_component = format!("realtime_final:{}", state.realtime_engine_name);
            match run_observed_blocking_inference_with_timeout(
                state.realtime_engine.clone(),
                request,
                state.realtime_final_timeout,
                state.metrics.clone(),
                "realtime_final",
            )
            .await
            {
                Ok(result) => {
                    state.runtime_health.record_success(health_component);
                    state.metrics.record_inference(
                        "realtime_final",
                        wait_elapsed,
                        inference_started_at.elapsed(),
                        audio_duration_ms,
                    );
                    let _ = queue_event_or_overflow(
                        state,
                        event_tx,
                        overflow_tx,
                        ServerEvent::final_text(result.text),
                    );
                }
                Err(error) => {
                    record_inference_health(state, &health_component, "realtime_final", &error);
                    state.metrics.record_inference(
                        "realtime_final",
                        wait_elapsed,
                        inference_started_at.elapsed(),
                        audio_duration_ms,
                    );
                    let _ = queue_event_or_overflow(
                        state,
                        event_tx,
                        overflow_tx,
                        map_realtime_inference_error(error, "realtime final inference timed out"),
                    );
                }
            }
            health_admission.cancel(state);
        }
        ActiveRealtimeSession::Native {
            session_id,
            health_admission,
            mut stream,
            bytes_received,
            ..
        } => {
            let _ = state.session_manager.finish_session(&session_id);
            state.metrics.dec_ws_sessions_active();
            let audio_duration_ms = pcm_duration_ms(bytes_received);
            let wait_started_at = Instant::now();
            let _inference_permit = state.scheduler.acquire_realtime_inference().await;
            let wait_elapsed = wait_started_at.elapsed();

            let inference_started_at = Instant::now();
            let health_component = format!("realtime_final:{}", state.realtime_engine_name);
            match stream.finish() {
                Ok(result) => {
                    state.runtime_health.record_success(health_component);
                    state.metrics.record_inference(
                        "realtime_final",
                        wait_elapsed,
                        inference_started_at.elapsed(),
                        audio_duration_ms,
                    );
                    let _ = queue_event_or_overflow(
                        state,
                        event_tx,
                        overflow_tx,
                        ServerEvent::final_text(result.text),
                    );
                }
                Err(error) => {
                    record_native_inference_health(
                        state,
                        &health_component,
                        "realtime_final",
                        &error,
                    );
                    state.metrics.record_inference(
                        "realtime_final",
                        wait_elapsed,
                        inference_started_at.elapsed(),
                        audio_duration_ms,
                    );
                    let _ = queue_event_or_overflow(
                        state,
                        event_tx,
                        overflow_tx,
                        ServerEvent::error("inference_failed", error.to_string()),
                    );
                }
            }
            health_admission.cancel(state);
        }
    }
}

fn queue_event_or_overflow(
    state: &AppState,
    event_tx: &mpsc::Sender<ServerEvent>,
    overflow_tx: &mpsc::Sender<()>,
    event: ServerEvent,
) -> bool {
    let error_code = event.code.clone();
    match queue_event(event_tx, event) {
        Ok(()) => {
            if let Some(code) = error_code {
                state.metrics.record_error(code);
            }
            true
        }
        Err(QueueEventError::Full) => {
            state.metrics.record_ws_queue_overflow();
            state.metrics.record_error("websocket_queue_overflow");
            let _ = queue_event(
                event_tx,
                ServerEvent::error(
                    "websocket_queue_overflow",
                    "websocket event queue overflowed",
                ),
            );
            let _ = overflow_tx.try_send(());
            false
        }
        Err(QueueEventError::Closed) => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueEventError {
    Full,
    Closed,
}

fn queue_event(
    event_tx: &mpsc::Sender<ServerEvent>,
    event: ServerEvent,
) -> Result<(), QueueEventError> {
    event_tx.try_send(event).map_err(|error| match error {
        TrySendError::Full(_) => QueueEventError::Full,
        TrySendError::Closed(_) => QueueEventError::Closed,
    })
}

fn map_realtime_inference_error(
    error: BlockingInferenceError,
    timeout_reason: &'static str,
) -> ServerEvent {
    match error {
        BlockingInferenceError::Timeout { .. } => {
            // The blocking OS thread may continue, but the client receives a bounded contract.
            ServerEvent::error("inference_timeout", timeout_reason)
        }
        BlockingInferenceError::Inference(error) => {
            ServerEvent::error("inference_failed", error.to_string())
        }
    }
}

fn record_inference_health(
    state: &AppState,
    component: &str,
    kind: &'static str,
    error: &BlockingInferenceError,
) {
    match error {
        BlockingInferenceError::Timeout { .. } => state
            .runtime_health
            .record_failure(component, format!("{kind} inference timeout")),
        BlockingInferenceError::Inference(InferenceError::Runtime(_)) => state
            .runtime_health
            .record_failure(component, format!("{kind} runtime inference error")),
        BlockingInferenceError::Inference(InferenceError::Unavailable(_)) => state
            .runtime_health
            .record_failure(component, format!("{kind} engine unavailable")),
        BlockingInferenceError::Inference(
            InferenceError::InvalidAudio(_)
            | InferenceError::UnsupportedEngine(_)
            | InferenceError::UnsupportedStreaming(_),
        ) => {}
    }
}

fn record_native_inference_health(
    state: &AppState,
    component: &str,
    kind: &'static str,
    error: &InferenceError,
) {
    match error {
        InferenceError::Runtime(_) => state
            .runtime_health
            .record_failure(component, format!("{kind} runtime inference error")),
        InferenceError::Unavailable(_) => state
            .runtime_health
            .record_failure(component, format!("{kind} engine unavailable")),
        InferenceError::InvalidAudio(_)
        | InferenceError::UnsupportedEngine(_)
        | InferenceError::UnsupportedStreaming(_) => {}
    }
}

#[derive(Default)]
struct RealtimeHealthAdmission {
    recovery_probe_components: Vec<String>,
}

impl RealtimeHealthAdmission {
    fn cancel(self, state: &AppState) {
        for component in self.recovery_probe_components {
            state.runtime_health.cancel_recovery_probe(&component);
        }
    }
}

fn admit_realtime_engine(state: &AppState) -> Result<RealtimeHealthAdmission, String> {
    let mut admission = RealtimeHealthAdmission::default();
    if !state.runtime_degraded_policy.fail_fast_inference() {
        return Ok(admission);
    }

    for component in [
        format!("realtime_partial:{}", state.realtime_engine_name),
        format!("realtime_final:{}", state.realtime_engine_name),
    ] {
        match state.runtime_health.admit_component(component.clone()) {
            crate::runtime_health::ComponentAdmission::Allowed => {}
            crate::runtime_health::ComponentAdmission::RecoveryProbe => {
                admission.recovery_probe_components.push(component);
            }
            crate::runtime_health::ComponentAdmission::Rejected => {
                admission.cancel(state);
                return Err(format!(
                    "speech engine degraded: realtime engine {} is degraded",
                    state.realtime_engine_name
                ));
            }
        }
    }

    Ok(admission)
}

fn cleanup_active_session(state: &AppState, active_session: &mut Option<ActiveRealtimeSession>) {
    if let Some(session) = active_session.take() {
        let (session_id, health_admission) = match session {
            ActiveRealtimeSession::Buffered {
                session_id,
                health_admission,
            }
            | ActiveRealtimeSession::Native {
                session_id,
                health_admission,
                ..
            } => (session_id, health_admission),
        };
        if state.session_manager.finish_session(&session_id).is_ok() {
            state.metrics.dec_ws_sessions_active();
        }
        health_admission.cancel(state);
    }
}

fn pcm_duration_ms(byte_len: usize) -> u64 {
    let sample_count = byte_len / PCM_BYTES_PER_SAMPLE;
    sample_count as u64 * 1000 / PCM_SAMPLE_RATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_event_reports_full_bounded_channel() {
        let (event_tx, _event_rx) = mpsc::channel(1);

        assert_eq!(
            queue_event(&event_tx, ServerEvent::partial_text("first")),
            Ok(())
        );
        assert_eq!(
            queue_event(&event_tx, ServerEvent::partial_text("second")),
            Err(QueueEventError::Full)
        );
    }
}
