use axum::Router;
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use crate::app::AppState;

#[derive(OpenApi)]
#[openapi(
    paths(health_ready_doc, transcribe_short_doc, realtime_doc),
    components(
        schemas(
            AudioBinaryBodyDoc,
            ClientEventDoc,
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
struct ShortAudioResultDoc {
    text: String,
    segments: Vec<TranscriptSegmentDoc>,
    detected_language: String,
    engine: String,
    duration_ms: u64,
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
        (status = 200, description = "Transcription completed", body = ShortAudioResultDoc),
        (status = 429, description = "Short-audio concurrency limit exceeded"),
        (status = 503, description = "Inference engine is unavailable")
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
            description = "WebSocket upgraded. Send {\"event\":\"start\"}, then raw pcm_s16le 16 kHz mono binary chunks, then {\"event\":\"stop\"}. Server emits session_started, partial, final, and error events."
        )
    )
)]
#[allow(dead_code)]
fn realtime_doc() {}

pub fn router() -> Router<AppState> {
    Router::<AppState>::new().merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()))
}
