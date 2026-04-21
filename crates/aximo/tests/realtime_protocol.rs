use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

async fn spawn_test_server() -> (String, tokio::task::JoinHandle<()>) {
    let app = aximo::app::build_test_app().await;
    spawn_server_with_app(app).await
}

async fn spawn_server_with_app(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("ws://{address}/v1/realtime"), handle)
}

async fn next_event(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let message = socket.next().await.unwrap().unwrap();
    let text = match message {
        Message::Text(value) => value,
        other => panic!("expected text message, got {other:?}"),
    };

    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn websocket_session_emits_started_and_final_events() {
    let (url, server) = spawn_test_server().await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();

    let started = next_event(&mut socket).await;
    let final_event = next_event(&mut socket).await;

    assert_eq!(started["event"], "session_started");
    assert_eq!(final_event["event"], "final");
    assert_eq!(final_event["text"], "hello world");

    server.abort();
}

#[tokio::test]
async fn websocket_session_returns_error_for_invalid_json() {
    let (url, server) = spawn_test_server().await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket.send(Message::Text("not-json".into())).await.unwrap();

    let event = next_event(&mut socket).await;
    assert_eq!(event["event"], "error");

    server.abort();
}

#[tokio::test]
async fn websocket_session_returns_error_for_binary_before_start() {
    let (url, server) = spawn_test_server().await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Binary(vec![0_u8; 3200]))
        .await
        .unwrap();

    let event = next_event(&mut socket).await;
    assert_eq!(event["event"], "error");

    server.abort();
}

#[tokio::test]
async fn websocket_session_returns_error_when_capacity_is_exhausted() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_realtime_sessions = 1;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let (url, server) = spawn_server_with_app(app).await;
    let (mut first_socket, _) = connect_async(url.clone()).await.unwrap();
    let (mut second_socket, _) = connect_async(url).await.unwrap();

    first_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    second_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();

    let first_event = next_event(&mut first_socket).await;
    let second_event = next_event(&mut second_socket).await;

    assert_eq!(first_event["event"], "session_started");
    assert_eq!(second_event["event"], "error");

    server.abort();
}

#[tokio::test]
async fn websocket_session_emits_partial_after_audio_chunk() {
    let (url, server) = spawn_test_server().await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    socket
        .send(Message::Binary(vec![0_u8; 3200]))
        .await
        .unwrap();
    socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();

    let started = next_event(&mut socket).await;
    let partial = next_event(&mut socket).await;
    let final_event = next_event(&mut socket).await;

    assert_eq!(started["event"], "session_started");
    assert_eq!(partial["event"], "partial");
    assert_eq!(partial["text"], "hello world");
    assert_eq!(final_event["event"], "final");

    server.abort();
}
