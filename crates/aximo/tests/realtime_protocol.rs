use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use tokio::net::TcpListener;
use tokio::{
    sync::oneshot,
    time::{sleep, timeout, Duration},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

struct RecordingEngine {
    requests: Arc<Mutex<Vec<usize>>>,
}

impl RecordingEngine {
    fn new() -> (Self, Arc<Mutex<Vec<usize>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                requests: Arc::clone(&requests),
            },
            requests,
        )
    }
}

impl aximo_inference::engine::SpeechEngine for RecordingEngine {
    fn transcribe_short(
        &self,
        request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        self.requests
            .lock()
            .unwrap()
            .push(request.audio_bytes.len());

        Ok(aximo_core::ShortAudioResult::new("recorded", "recording"))
    }
}

struct BlockingEngine {
    call_count: AtomicUsize,
    first_started_tx: Mutex<Option<oneshot::Sender<()>>>,
    release_first_rx: Mutex<Option<mpsc::Receiver<()>>>,
}

impl BlockingEngine {
    fn new() -> (Self, oneshot::Receiver<()>, mpsc::Sender<()>) {
        let (first_started_tx, first_started_rx) = oneshot::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();

        (
            Self {
                call_count: AtomicUsize::new(0),
                first_started_tx: Mutex::new(Some(first_started_tx)),
                release_first_rx: Mutex::new(Some(release_first_rx)),
            },
            first_started_rx,
            release_first_tx,
        )
    }
}

impl aximo_inference::engine::SpeechEngine for BlockingEngine {
    fn transcribe_short(
        &self,
        _request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        let call_number = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;

        if call_number == 1 {
            if let Some(tx) = self.first_started_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }

            if let Some(rx) = self.release_first_rx.lock().unwrap().take() {
                let _ = rx.recv();
            }
        }

        Ok(aximo_core::ShortAudioResult::new("blocked", "blocking"))
    }
}

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
    assert_eq!(event["code"], "invalid_client_event");
    assert_eq!(event["reason"], "failed to parse client event");

    server.abort();
}

#[tokio::test]
async fn websocket_session_returns_error_for_binary_before_start() {
    let (url, server) = spawn_test_server().await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Binary(vec![0_u8; 3200].into()))
        .await
        .unwrap();

    let event = next_event(&mut socket).await;
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], "no_active_session");
    assert_eq!(event["reason"], "binary audio received before start");

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
    assert_eq!(second_event["code"], "realtime_capacity_exhausted");
    assert_eq!(
        second_event["reason"],
        "realtime session capacity exhausted"
    );

    server.abort();
}

#[tokio::test]
async fn websocket_session_rejects_audio_after_session_limit() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_realtime_session_bytes = 3_200;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let (url, server) = spawn_server_with_app(app).await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();

    let started = next_event(&mut socket).await;
    assert_eq!(started["event"], "session_started");

    socket
        .send(Message::Binary(vec![0_u8; 6_400].into()))
        .await
        .unwrap();

    let oversized = next_event(&mut socket).await;
    assert_eq!(oversized["event"], "error");
    assert_eq!(oversized["code"], "realtime_session_too_large");

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();

    let restarted = next_event(&mut socket).await;
    assert_eq!(restarted["event"], "session_started");

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
        .send(Message::Binary(vec![0_u8; 3200].into()))
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

#[tokio::test]
async fn websocket_close_releases_realtime_capacity() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_realtime_sessions = 1;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let (url, server) = spawn_server_with_app(app).await;
    let (mut first_socket, _) = connect_async(url.clone()).await.unwrap();

    first_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();

    let started = next_event(&mut first_socket).await;
    assert_eq!(started["event"], "session_started");

    first_socket.close(None).await.unwrap();
    sleep(Duration::from_millis(50)).await;

    let (mut second_socket, _) = connect_async(url).await.unwrap();
    second_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();

    let second_started = next_event(&mut second_socket).await;
    assert_eq!(second_started["event"], "session_started");

    server.abort();
}

#[tokio::test]
async fn websocket_rejects_duplicate_start_without_leaking_capacity() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_realtime_sessions = 2;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let (url, server) = spawn_server_with_app(app).await;
    let (mut first_socket, _) = connect_async(url.clone()).await.unwrap();

    first_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    let started = next_event(&mut first_socket).await;
    assert_eq!(started["event"], "session_started");

    first_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    let duplicate_start = next_event(&mut first_socket).await;
    assert_eq!(duplicate_start["event"], "error");
    assert_eq!(duplicate_start["code"], "duplicate_start");
    assert_eq!(
        duplicate_start["reason"],
        "session already started for this socket"
    );

    first_socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();
    let final_event = next_event(&mut first_socket).await;
    assert_eq!(final_event["event"], "final");

    let (mut second_socket, _) = connect_async(url.clone()).await.unwrap();
    let (mut third_socket, _) = connect_async(url).await.unwrap();

    second_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    third_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();

    let second_started = next_event(&mut second_socket).await;
    let third_started = next_event(&mut third_socket).await;

    assert_eq!(second_started["event"], "session_started");
    assert_eq!(third_started["event"], "session_started");

    server.abort();
}

#[tokio::test]
async fn websocket_partial_transcription_uses_bounded_rolling_window() {
    let settings = aximo::config::Settings::default();
    let (engine, request_lengths) = RecordingEngine::new();
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(engine),
    );
    let (url, server) = spawn_server_with_app(app).await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    let started = next_event(&mut socket).await;
    assert_eq!(started["event"], "session_started");

    let chunk = vec![7_u8; 100_000];
    for _ in 0..3 {
        socket
            .send(Message::Binary(chunk.clone().into()))
            .await
            .unwrap();

        let partial = next_event(&mut socket).await;
        assert_eq!(partial["event"], "partial");
    }

    socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();
    let final_event = next_event(&mut socket).await;
    assert_eq!(final_event["event"], "final");

    let lengths = request_lengths.lock().unwrap().clone();
    assert_eq!(lengths.len(), 4);
    assert_eq!(lengths[0], 100_000);
    assert_eq!(lengths[1], 160_000);
    assert_eq!(lengths[2], 160_000);
    assert_eq!(lengths[3], 300_000);

    server.abort();
}

#[tokio::test]
async fn websocket_partial_waits_for_realtime_inference_slot_instead_of_skipping() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_realtime_sessions = 2;
    settings.limits.max_realtime_inferences = 1;
    let (engine, first_started_rx, release_first_tx) = BlockingEngine::new();
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(engine),
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

    let first_started = next_event(&mut first_socket).await;
    let second_started = next_event(&mut second_socket).await;
    assert_eq!(first_started["event"], "session_started");
    assert_eq!(second_started["event"], "session_started");

    first_socket
        .send(Message::Binary(vec![1_u8; 3200].into()))
        .await
        .unwrap();
    first_started_rx.await.unwrap();

    second_socket
        .send(Message::Binary(vec![2_u8; 3200].into()))
        .await
        .unwrap();

    assert!(
        timeout(Duration::from_millis(100), next_event(&mut second_socket))
            .await
            .is_err()
    );

    release_first_tx.send(()).unwrap();

    let first_partial = timeout(Duration::from_secs(1), next_event(&mut first_socket))
        .await
        .unwrap();
    let second_partial = timeout(Duration::from_secs(1), next_event(&mut second_socket))
        .await
        .unwrap();

    assert_eq!(first_partial["event"], "partial");
    assert_eq!(second_partial["event"], "partial");

    server.abort();
}

#[tokio::test]
async fn websocket_final_waits_for_realtime_inference_slot_instead_of_erroring() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_realtime_sessions = 2;
    settings.limits.max_realtime_inferences = 1;
    let (engine, first_started_rx, release_first_tx) = BlockingEngine::new();
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(engine),
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

    let first_started = next_event(&mut first_socket).await;
    let second_started = next_event(&mut second_socket).await;
    assert_eq!(first_started["event"], "session_started");
    assert_eq!(second_started["event"], "session_started");

    first_socket
        .send(Message::Binary(vec![1_u8; 3200].into()))
        .await
        .unwrap();
    first_started_rx.await.unwrap();

    second_socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();

    assert!(
        timeout(Duration::from_millis(100), next_event(&mut second_socket))
            .await
            .is_err()
    );

    release_first_tx.send(()).unwrap();

    let first_partial = timeout(Duration::from_secs(1), next_event(&mut first_socket))
        .await
        .unwrap();
    let second_final = timeout(Duration::from_secs(1), next_event(&mut second_socket))
        .await
        .unwrap();

    assert_eq!(first_partial["event"], "partial");
    assert_eq!(second_final["event"], "final");

    server.abort();
}
