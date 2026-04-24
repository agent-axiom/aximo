use axum::{
    body::{to_bytes, Body},
    http::Request,
};
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
use tower::ServiceExt;

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

struct BlockingRecordingEngine {
    requests: Arc<Mutex<Vec<usize>>>,
    call_count: AtomicUsize,
    first_started_tx: Mutex<Option<oneshot::Sender<()>>>,
    release_first_rx: Mutex<Option<mpsc::Receiver<()>>>,
}

type BlockingRecordingParts = (
    BlockingRecordingEngine,
    Arc<Mutex<Vec<usize>>>,
    oneshot::Receiver<()>,
    mpsc::Sender<()>,
);

impl BlockingRecordingEngine {
    fn new() -> BlockingRecordingParts {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (first_started_tx, first_started_rx) = oneshot::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();

        (
            Self {
                requests: Arc::clone(&requests),
                call_count: AtomicUsize::new(0),
                first_started_tx: Mutex::new(Some(first_started_tx)),
                release_first_rx: Mutex::new(Some(release_first_rx)),
            },
            requests,
            first_started_rx,
            release_first_tx,
        )
    }
}

impl aximo_inference::engine::SpeechEngine for BlockingRecordingEngine {
    fn transcribe_short(
        &self,
        request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        self.requests
            .lock()
            .unwrap()
            .push(request.audio_bytes.len());

        let call_number = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
        if call_number == 1 {
            if let Some(tx) = self.first_started_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }

            if let Some(rx) = self.release_first_rx.lock().unwrap().take() {
                let _ = rx.recv();
            }
        }

        Ok(aximo_core::ShortAudioResult::new(
            "recorded",
            "blocking-recording",
        ))
    }
}

struct SlowEngine {
    sleep_for: Duration,
}

impl aximo_inference::engine::SpeechEngine for SlowEngine {
    fn transcribe_short(
        &self,
        _request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        std::thread::sleep(self.sleep_for);
        Ok(aximo_core::ShortAudioResult::new("slow", "slow"))
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
async fn websocket_final_emits_timeout_error_when_inference_exceeds_budget() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.realtime_final_timeout_ms = 5;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(SlowEngine {
            sleep_for: Duration::from_millis(50),
        }),
    );
    let (url, server) = spawn_server_with_app(app).await;
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
    let error = next_event(&mut socket).await;

    assert_eq!(started["event"], "session_started");
    assert_eq!(error["event"], "error");
    assert_eq!(error["code"], "inference_timeout");
    assert_eq!(error["reason"], "realtime final inference timed out");

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
async fn websocket_session_rejects_odd_length_pcm_chunk() {
    let (url, server) = spawn_test_server().await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    let started = next_event(&mut socket).await;
    assert_eq!(started["event"], "session_started");

    socket
        .send(Message::Binary(vec![0_u8; 3].into()))
        .await
        .unwrap();

    let event = next_event(&mut socket).await;
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], "invalid_audio_chunk");
    assert_eq!(
        event["reason"],
        "pcm_s16le realtime chunks must be aligned to 16-bit samples"
    );

    server.abort();
}

#[tokio::test]
async fn websocket_session_returns_error_for_stop_before_start() {
    let (url, server) = spawn_test_server().await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();

    let event = next_event(&mut socket).await;
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], "no_active_session");
    assert_eq!(event["reason"], "stop requested without an active session");

    server.abort();
}

#[tokio::test]
async fn websocket_session_returns_error_for_unsupported_client_event() {
    let (url, server) = spawn_test_server().await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"rewind"}"#.into()))
        .await
        .unwrap();

    let event = next_event(&mut socket).await;
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], "unsupported_client_event");
    assert_eq!(event["reason"], "unsupported client event: rewind");

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
async fn websocket_queue_overflow_is_observable_from_realtime_connection() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.realtime_event_channel_capacity = 1;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::FakeEngine),
    );
    let (url, server) = spawn_server_with_app(app.clone()).await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    for _ in 0..256 {
        if socket
            .send(Message::Text(r#"{"event":"start"}"#.into()))
            .await
            .is_err()
        {
            break;
        }
    }

    let mut saw_overflow = false;
    let read_events = async {
        while let Some(message) = socket.next().await {
            let Ok(Message::Text(text)) = message else {
                break;
            };
            let event: Value = serde_json::from_str(&text).unwrap();
            if event["code"] == "websocket_queue_overflow" {
                saw_overflow = true;
                break;
            }
        }
    };
    let _ = timeout(Duration::from_secs(2), read_events).await;

    let metrics_response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(metrics_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let metrics = String::from_utf8(body.to_vec()).unwrap();

    assert!(
        saw_overflow || metrics.contains("aximo_ws_queue_overflows_total 1"),
        "expected websocket queue overflow event or metric, metrics:\n{metrics}",
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
    let mut settings = aximo::config::Settings::default();
    settings.limits.realtime_partial_min_chunk_bytes = 3_200;
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
        .send(Message::Binary(vec![0_u8; 3200].into()))
        .await
        .unwrap();
    let partial = next_event(&mut socket).await;
    assert_eq!(partial["event"], "partial");
    assert_eq!(partial["text"], "hello world");

    socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();
    let final_event = next_event(&mut socket).await;

    assert_eq!(final_event["event"], "final");

    server.abort();
}

#[tokio::test]
async fn websocket_partial_emits_timeout_error_when_inference_exceeds_budget() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.realtime_partial_min_chunk_bytes = 1;
    settings.limits.realtime_partial_min_interval_ms = 0;
    settings.limits.realtime_partial_timeout_ms = 5;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(SlowEngine {
            sleep_for: Duration::from_millis(50),
        }),
    );
    let (url, server) = spawn_server_with_app(app).await;
    let (mut socket, _) = connect_async(url).await.unwrap();

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    socket
        .send(Message::Binary(vec![0_u8; 3_200].into()))
        .await
        .unwrap();

    let started = next_event(&mut socket).await;
    let error = next_event(&mut socket).await;

    assert_eq!(started["event"], "session_started");
    assert_eq!(error["event"], "error");
    assert_eq!(error["code"], "inference_timeout");
    assert_eq!(error["reason"], "realtime partial inference timed out");

    server.abort();
}

#[tokio::test]
async fn websocket_partial_emits_structured_error_when_engine_fails() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.realtime_partial_min_chunk_bytes = 3_200;
    let app = aximo::app::build_app(
        settings,
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(aximo_inference::engine::UnavailableEngine::new(
            "missing model",
        )),
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
        .send(Message::Binary(vec![0_u8; 3_200].into()))
        .await
        .unwrap();

    let event = next_event(&mut socket).await;
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], "inference_failed");
    assert_eq!(event["reason"], "speech engine unavailable: missing model");

    server.abort();
}

#[tokio::test]
async fn websocket_partial_is_debounced_by_interval() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.realtime_partial_min_interval_ms = 10_000;
    settings.limits.realtime_partial_min_chunk_bytes = 3_200;
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

    for _ in 0..3 {
        socket
            .send(Message::Binary(vec![0_u8; 3_200].into()))
            .await
            .unwrap();
    }

    let partial = next_event(&mut socket).await;
    assert_eq!(partial["event"], "partial");

    assert!(timeout(Duration::from_millis(150), next_event(&mut socket))
        .await
        .is_err());

    server.abort();
}

#[tokio::test]
async fn websocket_partial_waits_for_minimum_audio_budget() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.realtime_partial_min_interval_ms = 0;
    settings.limits.realtime_partial_min_chunk_bytes = 6_400;
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
        .send(Message::Binary(vec![0_u8; 3_200].into()))
        .await
        .unwrap();

    assert!(timeout(Duration::from_millis(150), next_event(&mut socket))
        .await
        .is_err());

    socket
        .send(Message::Binary(vec![0_u8; 3_200].into()))
        .await
        .unwrap();

    let partial = next_event(&mut socket).await;
    assert_eq!(partial["event"], "partial");

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
    let mut settings = aximo::config::Settings::default();
    settings.limits.realtime_partial_min_interval_ms = 0;
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
    settings.limits.realtime_partial_min_chunk_bytes = 3_200;
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
    settings.limits.realtime_partial_min_chunk_bytes = 3_200;
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

#[tokio::test]
async fn websocket_partial_uses_latest_wins_backpressure() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_realtime_inferences = 1;
    settings.limits.realtime_partial_min_interval_ms = 0;
    settings.limits.realtime_partial_min_chunk_bytes = 3_200;
    let (engine, request_lengths, first_started_rx, release_first_tx) =
        BlockingRecordingEngine::new();
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

    socket
        .send(Message::Binary(vec![1_u8; 3_200].into()))
        .await
        .unwrap();
    first_started_rx.await.unwrap();

    for byte in [2_u8, 3_u8, 4_u8] {
        socket
            .send(Message::Binary(vec![byte; 3_200].into()))
            .await
            .unwrap();
    }

    assert!(timeout(Duration::from_millis(100), next_event(&mut socket))
        .await
        .is_err());

    release_first_tx.send(()).unwrap();

    let first_partial = timeout(Duration::from_secs(1), next_event(&mut socket))
        .await
        .unwrap();
    let second_partial = timeout(Duration::from_secs(1), next_event(&mut socket))
        .await
        .unwrap();

    assert_eq!(first_partial["event"], "partial");
    assert_eq!(second_partial["event"], "partial");
    assert!(timeout(Duration::from_millis(150), next_event(&mut socket))
        .await
        .is_err());

    let lengths = request_lengths.lock().unwrap().clone();
    assert_eq!(lengths, vec![3_200, 12_800]);

    server.abort();
}

#[tokio::test]
async fn websocket_skips_partial_inference_when_session_stops_while_waiting_for_slot() {
    let mut settings = aximo::config::Settings::default();
    settings.limits.max_realtime_sessions = 2;
    settings.limits.max_realtime_inferences = 1;
    settings.limits.realtime_partial_min_interval_ms = 0;
    settings.limits.realtime_partial_min_chunk_bytes = 3_200;
    let (engine, requests, first_started_rx, release_first_tx) = BlockingRecordingEngine::new();
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
    first_socket
        .send(Message::Binary(vec![0_u8; 3_200].into()))
        .await
        .unwrap();
    first_started_rx.await.unwrap();

    second_socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    second_socket
        .send(Message::Binary(vec![0_u8; 3_200].into()))
        .await
        .unwrap();
    sleep(Duration::from_millis(50)).await;
    second_socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await
        .unwrap();
    sleep(Duration::from_millis(50)).await;

    release_first_tx.send(()).unwrap();

    let second_started = next_event(&mut second_socket).await;
    let second_final = timeout(Duration::from_secs(1), next_event(&mut second_socket))
        .await
        .expect("second session should receive final after stale partial is skipped");

    assert_eq!(second_started["event"], "session_started");
    assert_eq!(second_final["event"], "final");
    assert_eq!(*requests.lock().unwrap(), vec![3_200, 3_200]);

    server.abort();
}
