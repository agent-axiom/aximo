use std::{
    sync::mpsc as std_mpsc,
    time::{Duration, Instant},
};

use aximo_core::{PartialSchedule, SessionError, ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::{InferenceError, StreamingSpeechSession};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, mpsc::error::TrySendError, oneshot, OwnedSemaphorePermit};

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
const NATIVE_STREAMING_COMMAND_CAPACITY: usize = 1;

#[derive(Clone)]
struct NativeStreamingWorker {
    sender: std_mpsc::SyncSender<NativeStreamingCommand>,
}

enum NativeStreamingCommand {
    Accept {
        chunk: Vec<u8>,
        scheduler_permit: OwnedSemaphorePermit,
        execution_permit: OwnedSemaphorePermit,
        metrics: crate::metrics::Metrics,
        response: oneshot::Sender<Result<Option<ShortAudioResult>, InferenceError>>,
    },
    Finish {
        scheduler_permit: OwnedSemaphorePermit,
        execution_permit: OwnedSemaphorePermit,
        metrics: crate::metrics::Metrics,
        response: oneshot::Sender<Result<ShortAudioResult, InferenceError>>,
    },
}

#[derive(Debug)]
enum NativeStreamingCallError {
    Timeout { timeout_ms: u64 },
    Inference(InferenceError),
    QueueFull,
    WorkerStopped,
}

struct NativeExecutionGuard {
    metrics: crate::metrics::Metrics,
}

impl NativeExecutionGuard {
    fn new(metrics: crate::metrics::Metrics) -> Self {
        metrics.inc_blocking_tasks_active();
        metrics.inc_model_executions_active();
        Self { metrics }
    }
}

impl Drop for NativeExecutionGuard {
    fn drop(&mut self) {
        self.metrics.dec_model_executions_active();
        self.metrics.dec_blocking_tasks_active();
    }
}

impl NativeStreamingWorker {
    fn start(stream: Box<dyn StreamingSpeechSession>) -> Result<Self, InferenceError> {
        let (sender, receiver) = std_mpsc::sync_channel(NATIVE_STREAMING_COMMAND_CAPACITY);
        std::thread::Builder::new()
            .name("aximo-native-streaming-worker".to_string())
            .spawn(move || native_streaming_worker_loop(stream, receiver))
            .map_err(|error| {
                InferenceError::Runtime(format!("failed to start native streaming worker: {error}"))
            })?;

        Ok(Self { sender })
    }

    async fn accept_pcm_chunk(
        &self,
        chunk: Vec<u8>,
        scheduler_permit: OwnedSemaphorePermit,
        execution_permit: OwnedSemaphorePermit,
        metrics: crate::metrics::Metrics,
        kind: &'static str,
        timeout_duration: Duration,
    ) -> Result<Option<ShortAudioResult>, NativeStreamingCallError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .try_send(NativeStreamingCommand::Accept {
                chunk,
                scheduler_permit,
                execution_permit,
                metrics: metrics.clone(),
                response,
            })
            .map_err(|error| match error {
                std_mpsc::TrySendError::Full(_) => NativeStreamingCallError::QueueFull,
                std_mpsc::TrySendError::Disconnected(_) => NativeStreamingCallError::WorkerStopped,
            })?;

        wait_native_streaming_response(receiver, metrics, kind, timeout_duration).await
    }

    async fn finish(
        self,
        scheduler_permit: OwnedSemaphorePermit,
        execution_permit: OwnedSemaphorePermit,
        metrics: crate::metrics::Metrics,
        kind: &'static str,
        timeout_duration: Duration,
    ) -> Result<ShortAudioResult, NativeStreamingCallError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .try_send(NativeStreamingCommand::Finish {
                scheduler_permit,
                execution_permit,
                metrics: metrics.clone(),
                response,
            })
            .map_err(|error| match error {
                std_mpsc::TrySendError::Full(_) => NativeStreamingCallError::QueueFull,
                std_mpsc::TrySendError::Disconnected(_) => NativeStreamingCallError::WorkerStopped,
            })?;

        wait_native_streaming_response(receiver, metrics, kind, timeout_duration).await
    }
}

async fn wait_native_streaming_response<T>(
    receiver: oneshot::Receiver<Result<T, InferenceError>>,
    metrics: crate::metrics::Metrics,
    kind: &'static str,
    timeout_duration: Duration,
) -> Result<T, NativeStreamingCallError> {
    let timeout_ms = timeout_duration.as_millis().try_into().unwrap_or(u64::MAX);
    match tokio::time::timeout(timeout_duration, receiver).await {
        Ok(Ok(Ok(result))) => Ok(result),
        Ok(Ok(Err(error))) => Err(NativeStreamingCallError::Inference(error)),
        Ok(Err(_)) => Err(NativeStreamingCallError::WorkerStopped),
        Err(_) => {
            metrics.record_inference_timeout(kind);
            Err(NativeStreamingCallError::Timeout { timeout_ms })
        }
    }
}

fn native_streaming_worker_loop(
    mut stream: Box<dyn StreamingSpeechSession>,
    receiver: std_mpsc::Receiver<NativeStreamingCommand>,
) {
    while let Ok(command) = receiver.recv() {
        match command {
            NativeStreamingCommand::Accept {
                chunk,
                scheduler_permit,
                execution_permit,
                metrics,
                response,
            } => {
                let _scheduler_permit = scheduler_permit;
                let _execution_permit = execution_permit;
                let _guard = NativeExecutionGuard::new(metrics);
                let _ = response.send(stream.accept_pcm_chunk(&chunk));
            }
            NativeStreamingCommand::Finish {
                scheduler_permit,
                execution_permit,
                metrics,
                response,
            } => {
                let _scheduler_permit = scheduler_permit;
                let _execution_permit = execution_permit;
                let _guard = NativeExecutionGuard::new(metrics);
                let _ = response.send(stream.finish());
                break;
            }
        }
    }
}

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
                                let native_worker = if state
                                    .realtime_engine
                                    .capabilities()
                                    .supports_native_streaming
                                {
                                    match start_native_streaming_worker(&state).await {
                                        Ok(worker) => {
                                            state.runtime_health.record_success(format!(
                                                "realtime_stream:{}",
                                                state.realtime_engine_name
                                            ));
                                            Some(worker)
                                        }
                                        Err(error) => {
                                            let _ =
                                                state.session_manager.finish_session(&session_id);
                                            record_native_streaming_call_health(
                                                &state,
                                                &format!(
                                                    "realtime_stream:{}",
                                                    state.realtime_engine_name
                                                ),
                                                "realtime_stream",
                                                &error,
                                            );
                                            health_admission.cancel(&state);
                                            queue_or_break!(ServerEvent::error(
                                                "streaming_start_failed",
                                                native_streaming_start_error_reason(error),
                                            ),);
                                            continue;
                                        }
                                    }
                                } else {
                                    None
                                };

                                state.metrics.inc_ws_sessions_active();
                                active_session = Some(match native_worker {
                                    Some(worker) => ActiveRealtimeSession::Native {
                                        session_id: session_id.clone(),
                                        health_admission,
                                        worker,
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
                    if !chunk.len().is_multiple_of(PCM_BYTES_PER_SAMPLE) {
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
                            worker,
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
                            let audio_duration_ms = pcm_duration_ms(chunk.len());
                            let worker = worker.clone();
                            let health_component =
                                format!("realtime_partial:{}", state.realtime_engine_name);
                            let wait_started_at = Instant::now();
                            let scheduler_permit =
                                state.scheduler.acquire_realtime_inference().await;
                            let wait_elapsed = wait_started_at.elapsed();
                            let model_wait_started_at = Instant::now();
                            let execution_permit = match tokio::time::timeout(
                                state.realtime_partial_timeout,
                                state.realtime_engine.acquire_execution_permit(),
                            )
                            .await
                            {
                                Ok(permit) => {
                                    state.metrics.record_model_execution_wait(
                                        "realtime_partial",
                                        model_wait_started_at.elapsed(),
                                    );
                                    permit
                                }
                                Err(_) => {
                                    state.metrics.record_model_execution_wait(
                                        "realtime_partial",
                                        model_wait_started_at.elapsed(),
                                    );
                                    state
                                        .metrics
                                        .record_model_execution_wait_timeout("realtime_partial");
                                    state.metrics.record_inference_timeout("realtime_partial");
                                    record_native_streaming_call_health(
                                        &state,
                                        &health_component,
                                        "realtime_partial",
                                        &NativeStreamingCallError::Timeout {
                                            timeout_ms: state
                                                .realtime_partial_timeout
                                                .as_millis()
                                                .try_into()
                                                .unwrap_or(u64::MAX),
                                        },
                                    );
                                    cleanup_active_session(&state, &mut active_session);
                                    queue_or_break!(ServerEvent::error(
                                        "inference_timeout",
                                        "realtime partial inference timed out",
                                    ),);
                                    continue;
                                }
                            };

                            let inference_started_at = Instant::now();
                            let native_result = worker
                                .accept_pcm_chunk(
                                    chunk.to_vec(),
                                    scheduler_permit,
                                    execution_permit,
                                    state.metrics.clone(),
                                    "realtime_partial",
                                    state.realtime_partial_timeout,
                                )
                                .await;
                            state.metrics.record_inference(
                                "realtime_partial",
                                wait_elapsed,
                                inference_started_at.elapsed(),
                                audio_duration_ms,
                            );

                            match native_result {
                                Ok(Some(result)) => {
                                    state.runtime_health.record_success(health_component);
                                    queue_or_break!(ServerEvent::partial_text(result.text));
                                }
                                Ok(None) => {
                                    state.runtime_health.record_success(health_component);
                                }
                                Err(error) => {
                                    record_native_streaming_call_health(
                                        &state,
                                        &health_component,
                                        "realtime_partial",
                                        &error,
                                    );
                                    cleanup_active_session(&state, &mut active_session);
                                    queue_or_break!(map_native_streaming_error(
                                        error,
                                        "realtime partial inference timed out",
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

async fn start_native_streaming_worker(
    state: &AppState,
) -> Result<NativeStreamingWorker, NativeStreamingCallError> {
    let kind = "realtime_stream";
    let wait_started_at = Instant::now();
    let scheduler_permit = state.scheduler.acquire_realtime_inference().await;
    let wait_elapsed = wait_started_at.elapsed();

    let model_wait_started_at = Instant::now();
    let execution_permit = match tokio::time::timeout(
        state.realtime_partial_timeout,
        state.realtime_engine.acquire_execution_permit(),
    )
    .await
    {
        Ok(permit) => {
            state
                .metrics
                .record_model_execution_wait(kind, model_wait_started_at.elapsed());
            permit
        }
        Err(_) => {
            state
                .metrics
                .record_model_execution_wait(kind, model_wait_started_at.elapsed());
            state.metrics.record_model_execution_wait_timeout(kind);
            state.metrics.record_inference_timeout(kind);
            return Err(NativeStreamingCallError::Timeout {
                timeout_ms: state
                    .realtime_partial_timeout
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX),
            });
        }
    };

    let engine = state.realtime_engine.engine();
    let metrics = state.metrics.clone();
    let task_metrics = metrics.clone();
    let inference_started_at = Instant::now();
    let task = tokio::task::spawn_blocking(move || {
        let _scheduler_permit = scheduler_permit;
        let _execution_permit = execution_permit;
        let _guard = NativeExecutionGuard::new(task_metrics);
        engine.start_streaming_session()
    });

    let result = match tokio::time::timeout(state.realtime_partial_timeout, task).await {
        Ok(Ok(result)) => result.map_err(NativeStreamingCallError::Inference),
        Ok(Err(error)) => Err(NativeStreamingCallError::Inference(
            InferenceError::Runtime(format!("native streaming start task failed: {error}")),
        )),
        Err(_) => {
            metrics.record_inference_timeout(kind);
            Err(NativeStreamingCallError::Timeout {
                timeout_ms: state
                    .realtime_partial_timeout
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX),
            })
        }
    };

    state
        .metrics
        .record_inference(kind, wait_elapsed, inference_started_at.elapsed(), 0);

    NativeStreamingWorker::start(result?).map_err(NativeStreamingCallError::Inference)
}

enum ActiveRealtimeSession {
    Buffered {
        session_id: String,
        health_admission: RealtimeHealthAdmission,
    },
    Native {
        session_id: String,
        health_admission: RealtimeHealthAdmission,
        worker: NativeStreamingWorker,
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
            worker,
            bytes_received,
            ..
        } => {
            let _ = state.session_manager.finish_session(&session_id);
            state.metrics.dec_ws_sessions_active();
            let audio_duration_ms = pcm_duration_ms(bytes_received);
            let wait_started_at = Instant::now();
            let inference_permit = state.scheduler.acquire_realtime_inference().await;
            let wait_elapsed = wait_started_at.elapsed();

            let inference_started_at = Instant::now();
            let health_component = format!("realtime_final:{}", state.realtime_engine_name);
            let model_wait_started_at = Instant::now();
            let execution_permit = match tokio::time::timeout(
                state.realtime_final_timeout,
                state.realtime_engine.acquire_execution_permit(),
            )
            .await
            {
                Ok(permit) => {
                    state.metrics.record_model_execution_wait(
                        "realtime_final",
                        model_wait_started_at.elapsed(),
                    );
                    permit
                }
                Err(_) => {
                    state.metrics.record_model_execution_wait(
                        "realtime_final",
                        model_wait_started_at.elapsed(),
                    );
                    state
                        .metrics
                        .record_model_execution_wait_timeout("realtime_final");
                    state.metrics.record_inference_timeout("realtime_final");
                    record_native_streaming_call_health(
                        state,
                        &health_component,
                        "realtime_final",
                        &NativeStreamingCallError::Timeout {
                            timeout_ms: state
                                .realtime_final_timeout
                                .as_millis()
                                .try_into()
                                .unwrap_or(u64::MAX),
                        },
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
                        ServerEvent::error(
                            "inference_timeout",
                            "realtime final inference timed out",
                        ),
                    );
                    health_admission.cancel(state);
                    return;
                }
            };

            match worker
                .finish(
                    inference_permit,
                    execution_permit,
                    state.metrics.clone(),
                    "realtime_final",
                    state.realtime_final_timeout,
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
                    record_native_streaming_call_health(
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
                        map_native_streaming_error(error, "realtime final inference timed out"),
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

fn map_native_streaming_error(
    error: NativeStreamingCallError,
    timeout_reason: &'static str,
) -> ServerEvent {
    match error {
        NativeStreamingCallError::Timeout { timeout_ms } => ServerEvent::error(
            "inference_timeout",
            format!("{timeout_reason} after {timeout_ms}ms"),
        ),
        NativeStreamingCallError::Inference(error) => {
            ServerEvent::error("inference_failed", error.to_string())
        }
        NativeStreamingCallError::QueueFull => ServerEvent::error(
            "native_streaming_backpressure",
            "native streaming worker queue is full",
        ),
        NativeStreamingCallError::WorkerStopped => ServerEvent::error(
            "inference_failed",
            "native streaming worker stopped before returning a result",
        ),
    }
}

fn native_streaming_start_error_reason(error: NativeStreamingCallError) -> String {
    match error {
        NativeStreamingCallError::Timeout { timeout_ms } => {
            format!("native streaming start timed out after {timeout_ms}ms")
        }
        NativeStreamingCallError::Inference(error) => error.to_string(),
        NativeStreamingCallError::QueueFull => "native streaming worker queue is full".to_string(),
        NativeStreamingCallError::WorkerStopped => {
            "native streaming worker stopped before returning a result".to_string()
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

fn record_native_streaming_call_health(
    state: &AppState,
    component: &str,
    kind: &'static str,
    error: &NativeStreamingCallError,
) {
    match error {
        NativeStreamingCallError::Timeout { .. } => state
            .runtime_health
            .record_failure(component, format!("{kind} inference timeout")),
        NativeStreamingCallError::Inference(error) => {
            record_native_inference_health(state, component, kind, error);
        }
        NativeStreamingCallError::WorkerStopped => state
            .runtime_health
            .record_failure(component, format!("{kind} worker stopped")),
        NativeStreamingCallError::QueueFull => {}
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

    let mut components = vec![
        format!("realtime_partial:{}", state.realtime_engine_name),
        format!("realtime_final:{}", state.realtime_engine_name),
    ];
    if state
        .realtime_engine
        .capabilities()
        .supports_native_streaming
    {
        components.push(format!("realtime_stream:{}", state.realtime_engine_name));
    }

    for component in components {
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
    use std::{
        sync::mpsc as std_mpsc,
        sync::Arc,
        time::{Duration, Instant},
    };

    use aximo_core::Scheduler;
    use aximo_inference::engine::FakeEngine;

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

    struct BlockingNativeSession {
        accept_started_tx: Option<oneshot::Sender<()>>,
        release_rx: Option<std_mpsc::Receiver<()>>,
        finish_release_rx: Option<std_mpsc::Receiver<()>>,
    }

    impl StreamingSpeechSession for BlockingNativeSession {
        fn accept_pcm_chunk(
            &mut self,
            _chunk: &[u8],
        ) -> Result<Option<aximo_core::ShortAudioResult>, InferenceError> {
            if let Some(started_tx) = self.accept_started_tx.take() {
                let _ = started_tx.send(());
            }
            if let Some(release_rx) = self.release_rx.take() {
                let _ = release_rx.recv();
            }

            Ok(None)
        }

        fn finish(&mut self) -> Result<aximo_core::ShortAudioResult, InferenceError> {
            if let Some(release_rx) = self.finish_release_rx.take() {
                let _ = release_rx.recv();
            }

            Ok(aximo_core::ShortAudioResult::new(
                "finished",
                "blocking-native-test",
            ))
        }
    }

    async fn realtime_scheduler_permit() -> OwnedSemaphorePermit {
        Scheduler::new(1, 1, 1, 1)
            .acquire_realtime_inference()
            .await
    }

    async fn model_execution_permit() -> OwnedSemaphorePermit {
        crate::engine_runtime::EngineRuntime::new(Arc::new(FakeEngine))
            .acquire_execution_permit()
            .await
    }

    #[tokio::test]
    async fn native_streaming_worker_timeout_does_not_wait_for_blocking_session() {
        let (release_tx, release_rx) = std_mpsc::channel();
        let worker = NativeStreamingWorker::start(Box::new(BlockingNativeSession {
            accept_started_tx: None,
            release_rx: Some(release_rx),
            finish_release_rx: None,
        }))
        .expect("native streaming worker starts");
        let started_at = Instant::now();

        let result = worker
            .accept_pcm_chunk(
                vec![0, 0],
                realtime_scheduler_permit().await,
                model_execution_permit().await,
                crate::metrics::Metrics::default(),
                "realtime_partial",
                Duration::from_millis(10),
            )
            .await;

        assert!(matches!(
            result,
            Err(NativeStreamingCallError::Timeout { .. })
        ));
        assert!(
            started_at.elapsed() < Duration::from_millis(100),
            "native worker timeout should not wait for the blocking session call"
        );

        let _ = release_tx.send(());
    }

    #[tokio::test]
    async fn native_streaming_worker_reports_queue_full_without_unbounded_commands() {
        let (release_tx, release_rx) = std_mpsc::channel();
        let (started_tx, started_rx) = oneshot::channel();
        let worker = NativeStreamingWorker::start(Box::new(BlockingNativeSession {
            accept_started_tx: Some(started_tx),
            release_rx: Some(release_rx),
            finish_release_rx: None,
        }))
        .expect("native streaming worker starts");

        let first_worker = worker.clone();
        let first = tokio::spawn(async move {
            first_worker
                .accept_pcm_chunk(
                    vec![0, 0],
                    realtime_scheduler_permit().await,
                    model_execution_permit().await,
                    crate::metrics::Metrics::default(),
                    "realtime_partial",
                    Duration::from_secs(5),
                )
                .await
        });
        started_rx.await.expect("first command starts");

        let second_worker = worker.clone();
        let second = tokio::spawn(async move {
            second_worker
                .accept_pcm_chunk(
                    vec![0, 0],
                    realtime_scheduler_permit().await,
                    model_execution_permit().await,
                    crate::metrics::Metrics::default(),
                    "realtime_partial",
                    Duration::from_secs(5),
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let result = worker
            .accept_pcm_chunk(
                vec![0, 0],
                realtime_scheduler_permit().await,
                model_execution_permit().await,
                crate::metrics::Metrics::default(),
                "realtime_partial",
                Duration::from_secs(5),
            )
            .await;

        assert!(matches!(result, Err(NativeStreamingCallError::QueueFull)));

        let _ = release_tx.send(());
        assert!(first.await.unwrap().is_ok());
        assert!(second.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn native_streaming_worker_finish_timeout_is_bounded() {
        let (release_tx, release_rx) = std_mpsc::channel();
        let worker = NativeStreamingWorker::start(Box::new(BlockingNativeSession {
            accept_started_tx: None,
            release_rx: None,
            finish_release_rx: Some(release_rx),
        }))
        .expect("native streaming worker starts");
        let started_at = Instant::now();

        let result = worker
            .finish(
                realtime_scheduler_permit().await,
                model_execution_permit().await,
                crate::metrics::Metrics::default(),
                "realtime_final",
                Duration::from_millis(10),
            )
            .await;

        assert!(matches!(
            result,
            Err(NativeStreamingCallError::Timeout { .. })
        ));
        assert!(
            started_at.elapsed() < Duration::from_millis(100),
            "finish timeout should not wait for the blocking native session"
        );

        let _ = release_tx.send(());
    }

    #[tokio::test]
    async fn native_streaming_worker_reports_stopped_worker() {
        let (sender, receiver) = std_mpsc::sync_channel(NATIVE_STREAMING_COMMAND_CAPACITY);
        drop(receiver);
        let worker = NativeStreamingWorker { sender };

        let result = worker
            .accept_pcm_chunk(
                vec![0, 0],
                realtime_scheduler_permit().await,
                model_execution_permit().await,
                crate::metrics::Metrics::default(),
                "realtime_partial",
                Duration::from_secs(1),
            )
            .await;

        assert!(matches!(
            result,
            Err(NativeStreamingCallError::WorkerStopped)
        ));
    }
}
