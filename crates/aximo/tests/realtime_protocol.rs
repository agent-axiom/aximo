use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

async fn spawn_test_server() -> (String, tokio::task::JoinHandle<()>) {
    let app = aximo::app::build_test_app().await;
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
