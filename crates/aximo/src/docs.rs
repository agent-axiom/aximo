use std::sync::{Arc, OnceLock};

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::{serve as serve_swagger_ui_asset, Config};

use crate::app::AppState;

const DOCS_INDEX_HTML: &str = include_str!("../static/docs/index.html");
const RECORDER_SCRIPT: &str = include_str!("../static/docs/aximo-recorder.js");
const RECORDER_STYLES: &str = include_str!("../static/docs/aximo-recorder.css");

#[derive(OpenApi)]
#[openapi(
    paths(health_ready_doc, transcribe_short_doc, realtime_doc),
    components(
        schemas(
            AudioBinaryBodyDoc,
            ClientEventDoc,
            ErrorResponseDoc,
            ServerEventDoc,
            ShortAudioResultDoc,
            TranscriptSegmentDoc
        )
    ),
    tags(
        (name = "system", description = "Health and service readiness endpoints."),
        (name = "stt", description = "Short-audio and realtime speech-to-text endpoints.")
    ),
    info(
        title = "Aximo API",
        description = "CPU-first STT microservice for Russian and English. Short audio uses HTTP POST and realtime streaming uses WebSocket with raw pcm_s16le 16 kHz mono chunks.",
        version = "0.1.0"
    )
)]
struct ApiDoc;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "Binary audio payload for short transcription requests.",
    value_type = String,
    format = Binary
)]
struct AudioBinaryBodyDoc(Vec<u8>);

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ErrorResponseDoc {
    /// Stable machine-readable error code.
    code: String,
    /// Human-readable error message.
    message: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ShortAudioResultDoc {
    /// Full transcript text for the request.
    text: String,
    /// Transcript segments when the current engine integration exposes
    /// segmentation; empty when unavailable.
    segments: Vec<TranscriptSegmentDoc>,
    /// Language detected by the model, or `null` when the current engine
    /// integration does not expose language detection.
    #[schema(nullable)]
    detected_language: Option<String>,
    /// Engine identifier that produced the transcription.
    engine: String,
    /// Measured input audio duration in milliseconds.
    duration_ms: u64,
    /// Measured processing time in milliseconds for the completed transcription.
    processing_ms: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct TranscriptSegmentDoc {
    start_ms: u64,
    end_ms: u64,
    text: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ClientEventDoc {
    #[schema(example = "start")]
    event: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ServerEventDoc {
    #[schema(example = "partial")]
    event: String,
    #[schema(nullable)]
    session_id: Option<String>,
    #[schema(nullable)]
    text: Option<String>,
    #[schema(nullable)]
    code: Option<String>,
    #[schema(nullable)]
    reason: Option<String>,
}

#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "system",
    responses((status = 200, description = "Service is ready"))
)]
#[allow(dead_code)]
fn health_ready_doc() {}

#[utoipa::path(
    post,
    path = "/v1/transcriptions",
    tag = "stt",
    request_body(
        content = AudioBinaryBodyDoc,
        content_type = "application/octet-stream",
        description = "Raw audio bytes. Set Content-Type to audio/wav, audio/pcm, or application/octet-stream."
    ),
    responses(
        (status = 200, description = "Transcription completed. Optional metadata remains null or empty when the active engine integration does not expose it.", body = ShortAudioResultDoc),
        (status = 400, description = "Client supplied invalid audio or engine selection", body = ErrorResponseDoc),
        (status = 429, description = "Short-audio request or inference concurrency limit exceeded", body = ErrorResponseDoc),
        (status = 500, description = "Inference runtime failed inside the service", body = ErrorResponseDoc),
        (status = 503, description = "Inference engine is unavailable", body = ErrorResponseDoc)
    )
)]
#[allow(dead_code)]
fn transcribe_short_doc() {}

#[utoipa::path(
    get,
    path = "/v1/realtime",
    tag = "stt",
    responses(
        (
            status = 101,
            description = "WebSocket upgraded. Send {\"event\":\"start\"}, then raw pcm_s16le 16 kHz mono binary chunks, then {\"event\":\"stop\"}. Server emits session_started, partial, final, and error events. Error events include machine-readable code and human-readable reason."
        )
    )
)]
#[allow(dead_code)]
fn realtime_doc() {}

fn swagger_config() -> Arc<Config<'static>> {
    static CONFIG: OnceLock<Arc<Config<'static>>> = OnceLock::new();

    CONFIG
        .get_or_init(|| {
            Arc::new(
                Config::new(["/openapi.json"])
                    .display_request_duration(true)
                    .filter(true)
                    .try_it_out_enabled(true)
                    .validator_url("none"),
            )
        })
        .clone()
}

async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

async fn docs_redirect() -> Redirect {
    Redirect::to("/docs/")
}

async fn docs_index() -> Html<&'static str> {
    Html(DOCS_INDEX_HTML)
}

async fn docs_assets(Path(path): Path<String>) -> Response {
    match path.as_str() {
        "aximo-recorder.js" => (
            [(
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            )],
            RECORDER_SCRIPT,
        )
            .into_response(),
        "aximo-recorder.css" => (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            RECORDER_STYLES,
        )
            .into_response(),
        other => match serve_swagger_ui_asset(other, swagger_config()) {
            Ok(Some(file)) => {
                ([(header::CONTENT_TYPE, file.content_type)], file.bytes).into_response()
            }
            Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        },
    }
}

pub fn router() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/openapi.json", get(openapi_json))
        .route("/docs", get(docs_redirect))
        .route("/docs/", get(docs_index))
        .route("/docs/{*rest}", get(docs_assets))
}
