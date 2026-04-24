use std::sync::Arc;

use aximo_audio::ShortAudioLimits;
use aximo_core::{RealtimePartialLimits, RealtimeSessionLimits, Scheduler, SessionManager};
use aximo_inference::engine::{FakeEngine, SpeechEngine};
use axum::{extract::DefaultBodyLimit, Router};

use crate::{config::Settings, http, ws};

#[derive(Clone)]
pub struct AppState {
    pub offline_engine: Arc<dyn SpeechEngine>,
    pub realtime_engine: Arc<dyn SpeechEngine>,
    pub session_manager: SessionManager,
    pub scheduler: Scheduler,
    pub short_audio_limits: ShortAudioLimits,
    pub realtime_session_limits: RealtimeSessionLimits,
    pub realtime_partial_limits: RealtimePartialLimits,
}

pub fn build_app(
    settings: Settings,
    offline_engine: Arc<dyn SpeechEngine>,
    realtime_engine: Arc<dyn SpeechEngine>,
) -> Router {
    let short_audio_body_limit = settings.limits.max_short_audio_bytes;
    let state = AppState {
        offline_engine,
        realtime_engine,
        session_manager: SessionManager::new(),
        scheduler: Scheduler::new(
            settings.limits.max_short_audio_requests,
            settings.limits.max_realtime_sessions,
            settings.limits.max_short_inferences,
            settings.limits.max_realtime_inferences,
        ),
        short_audio_limits: ShortAudioLimits {
            max_raw_pcm_bytes: settings.limits.max_short_raw_pcm_bytes,
            max_duration_ms: settings.limits.max_short_audio_duration_ms,
            max_decoded_samples: settings.limits.max_short_decoded_samples,
        },
        realtime_session_limits: RealtimeSessionLimits {
            max_bytes: settings.limits.max_realtime_session_bytes,
            max_duration: std::time::Duration::from_millis(
                settings.limits.max_realtime_session_duration_ms,
            ),
        },
        realtime_partial_limits: RealtimePartialLimits {
            min_interval: std::time::Duration::from_millis(
                settings.limits.realtime_partial_min_interval_ms,
            ),
            min_chunk_bytes: settings.limits.realtime_partial_min_chunk_bytes,
        },
    };

    Router::new()
        .route("/health/ready", axum::routing::get(http::health::ready))
        .route(
            "/v1/transcriptions",
            axum::routing::post(http::transcriptions::transcribe_short)
                .layer(DefaultBodyLimit::max(short_audio_body_limit)),
        )
        .route(
            "/v1/realtime",
            axum::routing::get(ws::handler::realtime_socket),
        )
        .merge(crate::docs::router())
        .with_state(state)
}

pub async fn build_test_app() -> Router {
    build_app(
        Settings::default(),
        Arc::new(FakeEngine),
        Arc::new(FakeEngine),
    )
}
