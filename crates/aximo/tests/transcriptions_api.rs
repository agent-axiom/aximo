use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use std::sync::Arc;

use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn transcription_endpoint_returns_fake_engine_result() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/wav")
                .body(Body::from(vec![0_u8; 3200]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["text"], "hello world");
    assert_eq!(json["engine"], "fake");
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
                .body(Body::from(vec![0_u8; 3200]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
