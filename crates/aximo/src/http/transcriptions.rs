use aximo_audio::AudioError;
use aximo_core::{ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::InferenceError;
use axum::{
    body::Bytes,
    extract::{
        rejection::{BytesRejection, QueryRejection},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::{
    app::AppState,
    inference_task::{run_observed_blocking_inference_with_timeout, BlockingInferenceError},
};

#[derive(Debug, Serialize)]
pub struct ErrorResponseBody {
    pub code: String,
    pub message: String,
}

pub struct HttpError {
    status: StatusCode,
    body: ErrorResponseBody,
}

#[derive(Debug, Default, Deserialize)]
pub struct TranscriptionOptions {
    engine: Option<String>,
    language: Option<String>,
    language_hint: Option<String>,
    timestamps: Option<bool>,
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
    query: Result<Query<TranscriptionOptions>, QueryRejection>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<ShortAudioResult>, HttpError> {
    let options = query
        .map(|Query(options)| options)
        .map_err(map_query_rejection)
        .inspect_err(|error| record_http_error(&state, error))?;
    let body = body
        .map_err(map_body_rejection)
        .inspect_err(|error| record_http_error(&state, error))?;
    state.metrics.record_audio_body_bytes(body.len());

    let _request_permit = state
        .scheduler
        .try_acquire_short_audio_request()
        .map_err(|_| {
            HttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "short_audio_request_capacity_exhausted",
                "short-audio request capacity exhausted",
            )
        })
        .inspect_err(|error| record_http_error(&state, error))?;

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let decode_started_at = Instant::now();
    let prepared_audio = aximo_audio::prepare_short_audio_with_limits(
        body.as_ref(),
        &content_type,
        state.short_audio_limits,
    )
    .map_err(map_audio_error)
    .inspect_err(|error| {
        state
            .metrics
            .record_audio_decode(decode_started_at.elapsed(), None);
        record_http_error(&state, error);
    })?;
    state.metrics.record_audio_decode(
        decode_started_at.elapsed(),
        Some(prepared_audio.duration_ms),
    );
    let audio_duration_ms = prepared_audio.duration_ms;
    let requested_engine = normalize_option(options.engine);
    if let Some(engine) = requested_engine.as_deref() {
        if engine != state.offline_engine_name {
            let error = HttpError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_engine",
                format!(
                    "unsupported engine: {engine}; configured short-audio engine is {}",
                    state.offline_engine_name
                ),
            );
            record_http_error(&state, &error);
            return Err(error);
        }
    }
    let language_hint =
        normalize_option(options.language_hint).or_else(|| normalize_option(options.language));

    let request = ShortAudioRequest {
        audio_bytes: prepared_audio.audio_bytes,
        content_type: prepared_audio.content_type.to_string(),
        engine: requested_engine,
        language_hint,
        timestamps: options.timestamps.unwrap_or(false),
    };

    let _inference_permit = state
        .scheduler
        .try_acquire_short_inference()
        .map_err(|_| {
            HttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "short_audio_inference_capacity_exhausted",
                "short-audio inference capacity exhausted",
            )
        })
        .inspect_err(|error| record_http_error(&state, error))?;
    let inference_started_at = Instant::now();
    let result = run_observed_blocking_inference_with_timeout(
        state.offline_engine.clone(),
        request,
        state.short_inference_timeout,
        state.metrics.clone(),
        "short",
    )
    .await;
    let inference_elapsed = inference_started_at.elapsed();
    state.metrics.record_inference(
        "short",
        Duration::ZERO,
        inference_elapsed,
        audio_duration_ms,
    );

    let health_component = format!("short:{}", state.offline_engine_name);
    match result {
        Ok(result) => {
            state.runtime_health.record_success(health_component);
            state.metrics.record_http_response(200, "ok");
            Ok(Json(result))
        }
        Err(error) => {
            record_inference_health(&state, &health_component, "short", &error);
            let error = map_blocking_inference_error(error, "short-audio inference timed out");
            record_http_error(&state, &error);
            Err(error)
        }
    }
}

fn normalize_option(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn map_query_rejection(error: QueryRejection) -> HttpError {
    HttpError::new(
        StatusCode::BAD_REQUEST,
        "invalid_query",
        format!(
            "invalid transcription query parameters: {}",
            error.body_text()
        ),
    )
}

fn map_body_rejection(error: BytesRejection) -> HttpError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return HttpError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "request body exceeds max_short_audio_bytes",
        );
    }

    HttpError::new(error.status(), "invalid_request_body", error.body_text())
}

fn map_audio_error(error: AudioError) -> HttpError {
    match error {
        AudioError::UnsupportedContentType(message) => HttpError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            format!("unsupported media type: {message}"),
        ),
        AudioError::TooLarge(message) => HttpError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            format!("audio payload too large: {message}"),
        ),
        AudioError::InvalidPcm(message) => HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_audio",
            format!("invalid audio payload: {message}"),
        ),
        AudioError::Decode(message) => HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_audio",
            format!("invalid audio payload: failed to decode audio container: {message}"),
        ),
    }
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

fn record_http_error(state: &AppState, error: &HttpError) {
    state
        .metrics
        .record_http_response(error.status.as_u16(), error.body.code.clone());
    state.metrics.record_error(error.body.code.clone());
}

fn map_blocking_inference_error(error: BlockingInferenceError, timeout_message: &str) -> HttpError {
    match error {
        BlockingInferenceError::Timeout { .. } => HttpError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "inference_timeout",
            timeout_message,
        ),
        BlockingInferenceError::Inference(error) => map_inference_error(error),
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
            InferenceError::InvalidAudio(_) | InferenceError::UnsupportedEngine(_),
        ) => {}
    }
}
