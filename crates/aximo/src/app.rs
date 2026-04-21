use axum::Router;

use crate::{config::Settings, http::health};

pub fn build_app(_settings: Settings) -> Router {
    Router::new().route("/health/ready", axum::routing::get(health::ready))
}

pub async fn build_test_app() -> Router {
    build_app(Settings::default())
}
