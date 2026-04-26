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
    assert!(json["paths"]["/v1/capabilities"].is_object());
    assert!(json["paths"]["/health/live"].is_object());
    assert!(json["paths"]["/health/ready"].is_object());
    assert!(json["paths"]["/health/ready"]["get"]["responses"]["503"].is_object());
    assert!(json["paths"]["/v1/transcriptions"]["post"]["responses"]["504"].is_object());
    assert!(json["paths"]["/v1/transcriptions"]["post"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|parameter| parameter["name"] == "engine"));
    assert!(json["components"]["schemas"]["ReadinessDoc"]["properties"]["components"].is_object());
    assert!(json["components"]["schemas"]["ComponentReadinessDoc"].is_object());
    assert!(json["components"]["schemas"]["CapabilitiesDoc"].is_object());
    assert!(json["components"]["schemas"]["EngineCapabilitiesDoc"].is_object());
    assert!(json["paths"]["/v1/realtime"].is_object());
}

#[tokio::test]
async fn openapi_realtime_description_names_buffering_and_partial_semantics() {
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
    let description = json["paths"]["/v1/realtime"]["get"]["responses"]["101"]["description"]
        .as_str()
        .unwrap();

    assert!(description.contains("bounded buffered realtime"));
    assert!(description.contains("supports_native_streaming=true"));
    assert!(description.contains("stateful native streaming session"));
    assert!(description.contains("bounded native streaming worker"));
    assert!(description.contains("engine_degraded"));
    assert!(description.contains("inference_timeout"));
    assert!(description.contains("latest-wins"));
    assert!(description.contains("full bounded session buffer"));
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
    assert!(html.contains("Aximo Recorder"));
    assert!(html.contains("Use microphone"));
    assert!(html.contains("Short Audio"));
    assert!(html.contains("Realtime"));
}

#[tokio::test]
async fn swagger_ui_redirects_to_trailing_slash() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/docs").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/docs/")
    );
}

#[tokio::test]
async fn recorder_script_is_served_as_javascript() {
    let app = aximo::app::build_test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/docs/aximo-recorder.js")
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
    let javascript = String::from_utf8(body.to_vec()).unwrap();

    assert!(content_type.starts_with("application/javascript"));
    assert!(javascript.contains("class AximoDocsRecorder"));
    assert!(javascript.contains("navigator.mediaDevices.getUserMedia"));
}

#[tokio::test]
async fn recorder_styles_and_swagger_assets_are_served() {
    let app = aximo::app::build_test_app().await;

    let recorder_styles = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/docs/aximo-recorder.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(recorder_styles.status(), StatusCode::OK);
    let recorder_content_type = recorder_styles
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let recorder_body = to_bytes(recorder_styles.into_body(), usize::MAX)
        .await
        .unwrap();
    let recorder_css = String::from_utf8(recorder_body.to_vec()).unwrap();

    assert!(recorder_content_type.starts_with("text/css"));
    assert!(recorder_css.contains(".aximo-recorder"));

    let swagger_styles = app
        .oneshot(
            Request::builder()
                .uri("/docs/swagger-ui.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(swagger_styles.status(), StatusCode::OK);
    let swagger_content_type = swagger_styles
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();

    assert!(swagger_content_type.starts_with("text/css"));
}
