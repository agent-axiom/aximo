use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use crate::app::AppState;

pub async fn live() -> StatusCode {
    StatusCode::OK
}

pub async fn ready(State(state): State<AppState>) -> Response {
    let readiness = state.runtime_health.readiness();
    let status = if state.runtime_health.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(readiness)).into_response()
}
