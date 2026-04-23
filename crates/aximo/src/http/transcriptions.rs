use aximo_core::{ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::InferenceError;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::{app::AppState, inference_task::run_blocking_inference};

#[derive(Debug, Serialize)]
pub struct ErrorResponseBody {
    pub code: String,
    pub message: String,
}

pub struct HttpError {
    status: StatusCode,
    body: ErrorResponseBody,
}

impl HttpError {
    fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ErrorResponseBody {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub async fn transcribe_short(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ShortAudioResult>, HttpError> {
    let _request_permit = state
        .scheduler
        .try_acquire_short_audio_request()
        .map_err(|_| {
            HttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "short_audio_request_capacity_exhausted",
                "short-audio request capacity exhausted",
            )
        })?;

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let request = ShortAudioRequest {
        audio_bytes: body.to_vec(),
        content_type,
        engine: None,
        language_hint: None,
        timestamps: false,
    };

    let _inference_permit = state.scheduler.try_acquire_short_inference().map_err(|_| {
        HttpError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "short_audio_inference_capacity_exhausted",
            "short-audio inference capacity exhausted",
        )
    })?;

    run_blocking_inference(state.offline_engine.clone(), request)
        .await
        .map(Json)
        .map_err(map_inference_error)
}

fn map_inference_error(error: InferenceError) -> HttpError {
    match error {
        InferenceError::UnsupportedEngine(message) => HttpError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_engine",
            format!("unsupported engine: {message}"),
        ),
        InferenceError::InvalidAudio(message) => HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_audio",
            format!("invalid audio payload: {message}"),
        ),
        InferenceError::Unavailable(message) => HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "engine_unavailable",
            format!("speech engine unavailable: {message}"),
        ),
        InferenceError::Runtime(message) => HttpError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "inference_runtime_error",
            format!("runtime inference error: {message}"),
        ),
    }
}
