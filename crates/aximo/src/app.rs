use std::sync::Arc;

use aximo_core::{Scheduler, SessionManager};
use aximo_inference::engine::{FakeEngine, SpeechEngine};
use axum::Router;

use crate::{config::Settings, http, ws};

#[derive(Clone)]
pub struct AppState {
    pub offline_engine: Arc<dyn SpeechEngine>,
    pub realtime_engine: Arc<dyn SpeechEngine>,
    pub session_manager: SessionManager,
    pub scheduler: Scheduler,
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
        ),
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
