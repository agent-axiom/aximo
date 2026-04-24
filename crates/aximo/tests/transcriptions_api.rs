use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{mpsc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

use serde_json::Value;
use tower::ServiceExt;

struct BlockingEngine {
    started_tx: Mutex<Option<oneshot::Sender<()>>>,
    release_rx: Mutex<Option<mpsc::Receiver<()>>>,
}

impl BlockingEngine {
    fn new() -> (Self, oneshot::Receiver<()>, mpsc::Sender<()>) {
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();

        (
            Self {
                started_tx: Mutex::new(Some(started_tx)),
                release_rx: Mutex::new(Some(release_rx)),
            },
            started_rx,
            release_tx,
        )
    }
}

impl aximo_inference::engine::SpeechEngine for BlockingEngine {
    fn transcribe_short(
        &self,
        _request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        if let Some(tx) = self.started_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }

        if let Some(rx) = self.release_rx.lock().unwrap().take() {
            let _ = rx.recv();
        }

        Ok(aximo_core::ShortAudioResult::new("done", "blocking"))
    }
}

struct StaticErrorEngine {
    error: aximo_inference::engine::InferenceError,
}

impl StaticErrorEngine {
    fn new(error: aximo_inference::engine::InferenceError) -> Self {
        Self { error }
    }
}

struct SlowEngine {
    sleep_for: Duration,
}

impl aximo_inference::engine::SpeechEngine for SlowEngine {
    fn transcribe_short(
        &self,
        _request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        std::thread::sleep(self.sleep_for);
        Ok(aximo_core::ShortAudioResult::new("slow", "slow"))
    }
}

impl aximo_inference::engine::SpeechEngine for StaticErrorEngine {
    fn transcribe_short(
        &self,
        _request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        Err(match &self.error {
            aximo_inference::engine::InferenceError::Unavailable(message) => {
                aximo_inference::engine::InferenceError::Unavailable(message.clone())
            }
            aximo_inference::engine::InferenceError::UnsupportedEngine(message) => {
                aximo_inference::engine::InferenceError::UnsupportedEngine(message.clone())
            }
            aximo_inference::engine::InferenceError::InvalidAudio(message) => {
                aximo_inference::engine::InferenceError::InvalidAudio(message.clone())
            }
            aximo_inference::engine::InferenceError::Runtime(message) => {
                aximo_inference::engine::InferenceError::Runtime(message.clone())
            }
        })
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let fixtures_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../aximo-audio/tests/fixtures");
    std::fs::read(fixtures_dir.join(name)).unwrap()
}

#[tokio::test]
async fn transcription_endpoint_returns_fake_engine_result() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/wav")
                .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["text"], "hello world");
    assert_eq!(json["segments"], serde_json::json!([]));
    assert_eq!(json["engine"], "fake");
    assert!(json["detected_language"].is_null());
    assert!(json["duration_ms"].is_number());
    assert!(json["processing_ms"].is_number());
}

#[tokio::test]
async fn transcription_endpoint_accepts_mp3_input() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/mpeg")
                .body(Body::from(fixture_bytes("tone-16k-mono.mp3")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn transcription_endpoint_accepts_wav_alias_with_parameters() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/x-wav; codecs=1")
                .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn transcription_endpoint_accepts_flac_input() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/flac")
                .body(Body::from(fixture_bytes("tone-16k-mono.flac")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn transcription_endpoint_accepts_m4a_input() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/mp4")
                .body(Body::from(fixture_bytes("tone-16k-mono.m4a")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn transcription_endpoint_returns_service_unavailable_when_engine_fails() {
    let app = aximo::app::build_app(
        aximo::config::Settings::default(),
        Arc::new(aximo_inference::engine::UnavailableEngine::new(
            "offline unavailable",
        )),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/wav")
                .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["code"], "engine_unavailable");
    assert_eq!(
        json["message"],
        "speech engine unavailable: offline unavailable"
    );
}

#[tokio::test]
async fn transcription_endpoint_returns_structured_capacity_error() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_short_audio_requests = 1;
    let (engine, started_rx, release_tx) = BlockingEngine::new();
    let app = aximo::app::build_app(
        settings,
        Arc::new(engine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );

    let first_request = Request::builder()
        .method("POST")
        .uri("/v1/transcriptions")
        .header("content-type", "audio/wav")
        .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
        .unwrap();
    let second_request = Request::builder()
        .method("POST")
        .uri("/v1/transcriptions")
        .header("content-type", "audio/wav")
        .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
        .unwrap();

    let app_for_first = app.clone();
    let first_handle =
        tokio::spawn(async move { app_for_first.oneshot(first_request).await.unwrap() });

    started_rx.await.unwrap();

    let second_response = app.oneshot(second_request).await.unwrap();
    assert_eq!(second_response.status(), StatusCode::TOO_MANY_REQUESTS);

    let second_body = to_bytes(second_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let second_json: Value = serde_json::from_slice(&second_body).unwrap();
    assert_eq!(
        second_json["code"],
        "short_audio_request_capacity_exhausted"
    );
    assert_eq!(
        second_json["message"],
        "short-audio request capacity exhausted"
    );

    release_tx.send(()).unwrap();

    let first_response = first_handle.await.unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn transcription_endpoint_returns_structured_inference_capacity_error() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_short_inferences = 1;
    let (engine, started_rx, release_tx) = BlockingEngine::new();
    let app = aximo::app::build_app(
        settings,
        Arc::new(engine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );

    let first_request = Request::builder()
        .method("POST")
        .uri("/v1/transcriptions")
        .header("content-type", "audio/wav")
        .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
        .unwrap();
    let second_request = Request::builder()
        .method("POST")
        .uri("/v1/transcriptions")
        .header("content-type", "audio/wav")
        .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
        .unwrap();

    let app_for_first = app.clone();
    let first_handle =
        tokio::spawn(async move { app_for_first.oneshot(first_request).await.unwrap() });

    started_rx.await.unwrap();

    let second_response = app.oneshot(second_request).await.unwrap();
    assert_eq!(second_response.status(), StatusCode::TOO_MANY_REQUESTS);

    let second_body = to_bytes(second_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let second_json: Value = serde_json::from_slice(&second_body).unwrap();
    assert_eq!(
        second_json["code"],
        "short_audio_inference_capacity_exhausted"
    );
    assert_eq!(
        second_json["message"],
        "short-audio inference capacity exhausted"
    );

    release_tx.send(()).unwrap();

    let first_response = first_handle.await.unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn transcription_endpoint_returns_gateway_timeout_when_inference_exceeds_budget() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.short_inference_timeout_ms = 5;
    let app = aximo::app::build_app(
        settings,
        Arc::new(SlowEngine {
            sleep_for: Duration::from_millis(50),
        }),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/wav")
                .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "inference_timeout");
    assert_eq!(json["message"], "short-audio inference timed out");
}

#[tokio::test]
async fn transcription_endpoint_returns_bad_request_for_invalid_audio() {
    let app = aximo::app::build_app(
        aximo::config::Settings::default(),
        Arc::new(StaticErrorEngine::new(
            aximo_inference::engine::InferenceError::InvalidAudio(
                "pcm payload must be aligned to 16-bit samples".to_string(),
            ),
        )),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/pcm")
                .body(Body::from(vec![1_u8, 2, 3]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["code"], "invalid_audio");
    assert_eq!(
        json["message"],
        "invalid audio payload: pcm payload must be aligned to 16-bit samples"
    );
}

#[tokio::test]
async fn transcription_endpoint_returns_unsupported_media_type_for_unknown_content_type() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "application/json")
                .body(Body::from(br#"{"audio":"nope"}"#.to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["code"], "unsupported_media_type");
    assert_eq!(json["message"], "unsupported media type: application/json");
}

#[tokio::test]
async fn transcription_endpoint_returns_unsupported_media_type_for_missing_content_type() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .body(Body::from(vec![0_u8; 4]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["code"], "unsupported_media_type");
    assert_eq!(
        json["message"],
        "unsupported media type: missing content type"
    );
}

#[tokio::test]
async fn transcription_endpoint_returns_payload_too_large_for_http_body_limit() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_short_audio_bytes = 4;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/pcm")
                .body(Body::from(vec![0_u8; 8]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "payload_too_large");
    assert_eq!(
        json["message"],
        "request body exceeds max_short_audio_bytes"
    );
}

#[tokio::test]
async fn transcription_endpoint_returns_payload_too_large_for_raw_pcm_limit() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_short_audio_bytes = 1024;
    settings.limits.max_short_raw_pcm_bytes = 4;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/pcm")
                .body(Body::from(vec![0_u8; 8]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "payload_too_large");
}

#[tokio::test]
async fn transcription_endpoint_returns_payload_too_large_for_decoded_duration_limit() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_short_audio_duration_ms = 1;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/wav")
                .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn transcription_endpoint_returns_payload_too_large_for_decoded_sample_limit() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_short_decoded_samples = 1;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/mpeg")
                .body(Body::from(fixture_bytes("tone-16k-mono.mp3")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn transcription_endpoint_returns_bad_request_for_unsupported_engine() {
    let app = aximo::app::build_app(
        aximo::config::Settings::default(),
        Arc::new(StaticErrorEngine::new(
            aximo_inference::engine::InferenceError::UnsupportedEngine("moonshine".to_string()),
        )),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/wav")
                .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["code"], "unsupported_engine");
    assert_eq!(json["message"], "unsupported engine: moonshine");
}

#[tokio::test]
async fn transcription_endpoint_returns_internal_error_for_runtime_failure() {
    let app = aximo::app::build_app(
        aximo::config::Settings::default(),
        Arc::new(StaticErrorEngine::new(
            aximo_inference::engine::InferenceError::Runtime(
                "blocking inference task failed".to_string(),
            ),
        )),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/wav")
                .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["code"], "inference_runtime_error");
    assert_eq!(
        json["message"],
        "runtime inference error: blocking inference task failed"
    );
}
