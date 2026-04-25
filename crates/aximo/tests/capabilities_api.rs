use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn capabilities_endpoint_reports_engine_metadata_and_realtime_mode() {
    let app = aximo::app::build_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/capabilities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["offline"]["configured_engine"], "parakeet");
    assert_eq!(json["offline"]["model"]["engine"], "fake");
    assert_eq!(
        json["offline"]["model"]["languages"],
        serde_json::json!(["en", "ru"])
    );
    assert_eq!(json["offline"]["model"]["supports_timestamps"], true);
    assert_eq!(
        json["offline"]["model"]["supports_language_detection"],
        false
    );
    assert_eq!(json["offline"]["model"]["supports_native_streaming"], false);
    assert_eq!(json["realtime"]["mode"], "bounded_buffered_offline");
}
