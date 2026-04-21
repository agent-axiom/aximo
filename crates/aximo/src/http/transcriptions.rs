use aximo_core::{ShortAudioRequest, ShortAudioResult};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::app::AppState;

pub async fn transcribe_short(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ShortAudioResult>, StatusCode> {
    let _permit = state
        .scheduler
        .try_acquire_short_audio()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;

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

    state
        .speech_engine
        .transcribe_short(request)
        .map(Json)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
