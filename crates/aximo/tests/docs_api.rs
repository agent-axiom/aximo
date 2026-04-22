use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn openapi_document_is_served_as_json() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["openapi"], "3.1.0");
    assert!(json["paths"]["/v1/transcriptions"].is_object());
    assert!(json["paths"]["/health/ready"].is_object());
    assert!(json["paths"]["/v1/realtime"].is_object());
}

#[tokio::test]
async fn swagger_ui_is_served_as_html() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/docs/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(content_type.starts_with("text/html"));
    assert!(html.contains("Swagger UI"));
}
