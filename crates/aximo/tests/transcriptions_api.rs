use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
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
