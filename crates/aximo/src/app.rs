use std::sync::Arc;

use aximo_core::SessionManager;
use aximo_inference::engine::{FakeEngine, SpeechEngine};
use axum::Router;

use crate::{config::Settings, http, ws};

#[derive(Clone)]
pub struct AppState {
    pub speech_engine: Arc<dyn SpeechEngine>,
    pub session_manager: SessionManager,
}

pub fn build_app(_settings: Settings, speech_engine: Arc<dyn SpeechEngine>) -> Router {
    let state = AppState {
        speech_engine,
        session_manager: SessionManager::new(),
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
        .with_state(state)
}

pub async fn build_test_app() -> Router {
    build_app(Settings::default(), Arc::new(FakeEngine))
}
