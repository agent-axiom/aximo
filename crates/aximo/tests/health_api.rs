use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
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

async fn spawn_server_with_app(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("ws://{address}/v1/realtime"), handle)
}

async fn next_ws_event(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let message = socket.next().await.unwrap().unwrap();
    let text = match message {
        Message::Text(value) => value,
        other => panic!("expected text websocket message, got {other:?}"),
    };

    serde_json::from_str(&text).unwrap()
}

fn component<'a>(json: &'a Value, name: &str) -> &'a Value {
    json["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["component"] == name)
        .unwrap_or_else(|| panic!("missing component {name} in readiness payload: {json}"))
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
    let short = component(&json, "short:parakeet");
    assert_eq!(short["status"], "degraded");
    assert_eq!(short["consecutive_failures"], 1);
    assert_eq!(short["reason"], "short runtime inference error");
}

#[tokio::test]
async fn realtime_success_does_not_clear_degraded_short_component() {
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

    let (url, server) = spawn_server_with_app(app.clone()).await;
    let (mut socket, _) = connect_async(url).await.unwrap();
    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();
    assert_eq!(next_ws_event(&mut socket).await["event"], "session_started");
    assert_eq!(next_ws_event(&mut socket).await["event"], "final");
    server.abort();

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
    assert_eq!(component(&json, "short:parakeet")["status"], "degraded");
    assert_eq!(
        component(&json, "realtime_final:parakeet")["status"],
        "ready"
    );
}
