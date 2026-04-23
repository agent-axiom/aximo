use std::sync::Arc;

use aximo_core::{RealtimeSessionLimits, Scheduler, SessionManager};
use aximo_inference::engine::{FakeEngine, SpeechEngine};
use axum::Router;

use crate::{config::Settings, http, ws};

#[derive(Clone)]
pub struct AppState {
    pub offline_engine: Arc<dyn SpeechEngine>,
    pub realtime_engine: Arc<dyn SpeechEngine>,
    pub session_manager: SessionManager,
    pub scheduler: Scheduler,
    pub realtime_session_limits: RealtimeSessionLimits,
}

pub fn build_app(
    settings: Settings,
    offline_engine: Arc<dyn SpeechEngine>,
    realtime_engine: Arc<dyn SpeechEngine>,
) -> Router {
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
        realtime_session_limits: RealtimeSessionLimits {
            max_bytes: settings.limits.max_realtime_session_bytes,
            max_duration: std::time::Duration::from_millis(
                settings.limits.max_realtime_session_duration_ms,
            ),
        },
    };

    Router::new()
        .route("/health/ready", axum::routing::get(http::health::ready))
        .route(
            "/v1/transcriptions",
            axum::routing::post(http::transcriptions::transcribe_short),
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
