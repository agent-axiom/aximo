use std::time::{Duration, Instant};

use aximo_core::{PartialSchedule, SessionError, ShortAudioRequest};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, mpsc::error::TrySendError};

use crate::{
    app::AppState,
    inference_task::{run_blocking_inference_with_timeout, BlockingInferenceError},
    ws::protocol::{ClientEvent, ServerEvent},
};

// 5 seconds of pcm_s16le 16 kHz mono audio.
const REALTIME_PARTIAL_WINDOW_BYTES: usize = 160_000;
const PCM_SAMPLE_RATE: u64 = 16_000;
const PCM_BYTES_PER_SAMPLE: usize = 2;
const WRITER_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);

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
    let mut writer = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let message = serde_json::to_string(&event).expect("serialize websocket event");
            if sender.send(Message::Text(message.into())).await.is_err() {
                break;
            }
        }
    });
    let mut active_session_id: Option<String> = None;

    loop {
        macro_rules! queue_or_break {
            ($event:expr $(,)?) => {
                if !queue_event_or_overflow(&state, &event_tx, &overflow_tx, $event) {
                    break;
                }
            };
        }

        let message = tokio::select! {
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
                        if active_session_id.is_some() {
                            queue_or_break!(ServerEvent::error(
                                "duplicate_start",
                                "session already started for this socket",
                            ),);
                            continue;
                        }

                        match state.scheduler.try_acquire_realtime_session() {
                            Ok(permit) => {
                                let session_id = state
                                    .session_manager
                                    .start_session(permit, state.realtime_session_limits);
                                state.metrics.inc_ws_sessions_active();
                                active_session_id = Some(session_id.clone());
                                queue_or_break!(ServerEvent::session_started(session_id));
                            }
                            Err(_) => {
                                queue_or_break!(ServerEvent::error(
                                    "realtime_capacity_exhausted",
                                    "realtime session capacity exhausted",
                                ),);
                            }
                        }
                    }
                    "stop" => {
                        if let Some(session_id) = active_session_id.take() {
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
                            let _inference_permit =
                                state.scheduler.acquire_realtime_inference().await;
                            let wait_elapsed = wait_started_at.elapsed();

                            let inference_started_at = Instant::now();
                            match run_blocking_inference_with_timeout(
                                state.realtime_engine.clone(),
                                request,
                                state.realtime_final_timeout,
                            )
                            .await
                            {
                                Ok(result) => {
                                    state.metrics.record_inference(
                                        "realtime_final",
                                        wait_elapsed,
                                        inference_started_at.elapsed(),
                                        audio_duration_ms,
                                    );
                                    queue_or_break!(ServerEvent::final_text(result.text));
                                }
                                Err(error) => {
                                    state.metrics.record_inference(
                                        "realtime_final",
                                        wait_elapsed,
                                        inference_started_at.elapsed(),
                                        audio_duration_ms,
                                    );
                                    queue_or_break!(map_realtime_inference_error(
                                        error,
                                        "realtime final inference timed out"
                                    ),);
                                }
                            }
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
                if let Some(session_id) = active_session_id.as_deref() {
                    match state.session_manager.append_audio(session_id, &chunk) {
                        Ok(()) => {
                            let schedule = state
                                .session_manager
                                .maybe_begin_partial(session_id, state.realtime_partial_limits)
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
                            cleanup_active_session(&state, &mut active_session_id);
                            queue_or_break!(ServerEvent::error(
                                "realtime_session_too_large",
                                "realtime session exceeded configured byte limit",
                            ),);
                        }
                        Err(SessionError::SessionTooLong) => {
                            cleanup_active_session(&state, &mut active_session_id);
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

    cleanup_active_session(&state, &mut active_session_id);
    drop(event_tx);
    tokio::select! {
        _ = &mut writer => {}
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
            let inference_result = run_blocking_inference_with_timeout(
                state.realtime_engine.clone(),
                request,
                state.realtime_partial_timeout,
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
                    if !queue_event_or_overflow(
                        &state,
                        &event_tx,
                        &overflow_tx,
                        map_realtime_inference_error(
                            error,
                            "realtime partial inference timed out",
                        ),
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

fn cleanup_active_session(state: &AppState, active_session_id: &mut Option<String>) {
    if let Some(session_id) = active_session_id.take() {
        if state.session_manager.finish_session(&session_id).is_ok() {
            state.metrics.dec_ws_sessions_active();
        }
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

        assert_eq!(queue_event(&event_tx, ServerEvent::partial_text("first")), Ok(()));
        assert_eq!(
            queue_event(&event_tx, ServerEvent::partial_text("second")),
            Err(QueueEventError::Full)
        );
    }
}
