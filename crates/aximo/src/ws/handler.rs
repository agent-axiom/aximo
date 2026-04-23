use aximo_core::ShortAudioRequest;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};

use crate::{
    app::AppState,
    inference_task::run_blocking_inference,
    ws::protocol::{ClientEvent, ServerEvent},
};

// 5 seconds of pcm_s16le 16 kHz mono audio.
const REALTIME_PARTIAL_WINDOW_BYTES: usize = 160_000;

pub async fn realtime_socket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut active_session_id: Option<String> = None;

    while let Some(Ok(message)) = socket.recv().await {
        match message {
            Message::Text(text) => {
                let Ok(client_event) = serde_json::from_str::<ClientEvent>(&text) else {
                    let _ = send_event(
                        &mut socket,
                        ServerEvent::error("invalid_client_event", "failed to parse client event"),
                    )
                    .await;
                    continue;
                };

                match client_event.event.as_str() {
                    "start" => {
                        if active_session_id.is_some() {
                            let _ = send_event(
                                &mut socket,
                                ServerEvent::error(
                                    "duplicate_start",
                                    "session already started for this socket",
                                ),
                            )
                            .await;
                            continue;
                        }

                        match state.scheduler.try_acquire_realtime_session() {
                            Ok(permit) => {
                                let session_id = state
                                    .session_manager
                                    .start_session(permit, state.realtime_session_limits);
                                active_session_id = Some(session_id.clone());
                                let _ = send_event(
                                    &mut socket,
                                    ServerEvent::session_started(session_id),
                                )
                                .await;
                            }
                            Err(_) => {
                                let _ = send_event(
                                    &mut socket,
                                    ServerEvent::error(
                                        "realtime_capacity_exhausted",
                                        "realtime session capacity exhausted",
                                    ),
                                )
                                .await;
                            }
                        }
                    }
                    "stop" => {
                        if let Some(session_id) = active_session_id.take() {
                            let audio_bytes = state
                                .session_manager
                                .finish_session(&session_id)
                                .unwrap_or_default();
                            let request = ShortAudioRequest {
                                audio_bytes,
                                content_type: "audio/pcm".to_string(),
                                engine: None,
                                language_hint: None,
                                timestamps: false,
                            };

                            let _inference_permit =
                                state.scheduler.acquire_realtime_inference().await;

                            match run_blocking_inference(state.realtime_engine.clone(), request)
                                .await
                            {
                                Ok(result) => {
                                    let _ = send_event(
                                        &mut socket,
                                        ServerEvent::final_text(result.text),
                                    )
                                    .await;
                                }
                                Err(error) => {
                                    let _ = send_event(
                                        &mut socket,
                                        ServerEvent::error("inference_failed", error.to_string()),
                                    )
                                    .await;
                                }
                            }
                        } else {
                            let _ = send_event(
                                &mut socket,
                                ServerEvent::error(
                                    "no_active_session",
                                    "stop requested without an active session",
                                ),
                            )
                            .await;
                        }
                    }
                    _ => {
                        let _ = send_event(
                            &mut socket,
                            ServerEvent::error(
                                "unsupported_client_event",
                                format!("unsupported client event: {}", client_event.event),
                            ),
                        )
                        .await;
                    }
                }
            }
            Message::Binary(chunk) => {
                if let Some(session_id) = active_session_id.as_deref() {
                    match state.session_manager.append_audio(session_id, &chunk) {
                        Ok(()) => {
                            let audio_bytes = state
                                .session_manager
                                .recent_audio_snapshot(session_id, REALTIME_PARTIAL_WINDOW_BYTES)
                                .unwrap_or_default();
                            let request = ShortAudioRequest {
                                audio_bytes,
                                content_type: "audio/pcm".to_string(),
                                engine: None,
                                language_hint: None,
                                timestamps: false,
                            };

                            let _inference_permit =
                                state.scheduler.acquire_realtime_inference().await;

                            match run_blocking_inference(state.realtime_engine.clone(), request)
                                .await
                            {
                                Ok(result) => {
                                    let _ = send_event(
                                        &mut socket,
                                        ServerEvent::partial_text(result.text),
                                    )
                                    .await;
                                }
                                Err(error) => {
                                    let _ = send_event(
                                        &mut socket,
                                        ServerEvent::error("inference_failed", error.to_string()),
                                    )
                                    .await;
                                }
                            }
                        }
                        Err(aximo_core::SessionError::SessionTooLarge) => {
                            cleanup_active_session(&state, &mut active_session_id);
                            let _ = send_event(
                                &mut socket,
                                ServerEvent::error(
                                    "realtime_session_too_large",
                                    "realtime session exceeded configured byte limit",
                                ),
                            )
                            .await;
                        }
                        Err(aximo_core::SessionError::SessionTooLong) => {
                            cleanup_active_session(&state, &mut active_session_id);
                            let _ = send_event(
                                &mut socket,
                                ServerEvent::error(
                                    "realtime_session_too_long",
                                    "realtime session exceeded configured duration limit",
                                ),
                            )
                            .await;
                        }
                        Err(aximo_core::SessionError::MissingSession) => {
                            let _ = send_event(
                                &mut socket,
                                ServerEvent::error(
                                    "audio_append_failed",
                                    "failed to append audio to the active realtime session",
                                ),
                            )
                            .await;
                        }
                    }
                } else {
                    let _ = send_event(
                        &mut socket,
                        ServerEvent::error(
                            "no_active_session",
                            "binary audio received before start",
                        ),
                    )
                    .await;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    cleanup_active_session(&state, &mut active_session_id);
}

async fn send_event(socket: &mut WebSocket, event: ServerEvent) -> Result<(), axum::Error> {
    let message = serde_json::to_string(&event).expect("serialize websocket event");
    socket.send(Message::Text(message.into())).await
}

fn cleanup_active_session(state: &AppState, active_session_id: &mut Option<String>) {
    if let Some(session_id) = active_session_id.take() {
        let _ = state.session_manager.finish_session(&session_id);
    }
}
