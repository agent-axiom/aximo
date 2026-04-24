use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

struct RuntimeErrorEngine;

impl aximo_inference::engine::SpeechEngine for RuntimeErrorEngine {
    fn transcribe_short(
        &self,
        _request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        Err(aximo_inference::engine::InferenceError::Runtime(
            "model execution failed".to_string(),
        ))
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let fixtures_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../aximo-audio/tests/fixtures");
    std::fs::read(fixtures_dir.join(name)).unwrap()
}

#[tokio::test]
async fn readiness_endpoint_returns_ok() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn liveness_endpoint_returns_ok() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn readiness_endpoint_reports_degraded_after_repeated_runtime_failures() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.runtime_degrade_after_consecutive_failures = 1;
    let app = aximo::app::build_app(
        settings,
        Arc::new(RuntimeErrorEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );

    let response = app
        .clone()
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

    let ready_response = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(ready_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(ready_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "degraded");
    assert_eq!(json["consecutive_failures"], 1);
    assert_eq!(json["reason"], "short runtime inference error");
}
