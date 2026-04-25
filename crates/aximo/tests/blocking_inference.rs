use std::{
    sync::atomic::{AtomicUsize, Ordering},
    sync::mpsc,
    sync::{Arc, Mutex},
    thread::ThreadId,
    time::Duration,
};

use aximo_core::{ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::{InferenceError, SpeechEngine};
use tokio::sync::oneshot;

struct ThreadRecordingEngine {
    seen_thread_id: Arc<Mutex<Option<ThreadId>>>,
}

impl ThreadRecordingEngine {
    fn new() -> (Self, Arc<Mutex<Option<ThreadId>>>) {
        let seen_thread_id = Arc::new(Mutex::new(None));
        (
            Self {
                seen_thread_id: Arc::clone(&seen_thread_id),
            },
            seen_thread_id,
        )
    }
}

impl SpeechEngine for ThreadRecordingEngine {
    fn transcribe_short(
        &self,
        _request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError> {
        *self.seen_thread_id.lock().unwrap() = Some(std::thread::current().id());
        Ok(ShortAudioResult::new("offloaded", "thread-recording"))
    }
}

struct BlockingGateEngine {
    call_count: Arc<AtomicUsize>,
    first_started_tx: Mutex<Option<oneshot::Sender<()>>>,
    release_first_rx: Mutex<Option<mpsc::Receiver<()>>>,
}

impl BlockingGateEngine {
    fn new() -> (
        Self,
        Arc<AtomicUsize>,
        oneshot::Receiver<()>,
        mpsc::Sender<()>,
    ) {
        let call_count = Arc::new(AtomicUsize::new(0));
        let (first_started_tx, first_started_rx) = oneshot::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();

        (
            Self {
                call_count: Arc::clone(&call_count),
                first_started_tx: Mutex::new(Some(first_started_tx)),
                release_first_rx: Mutex::new(Some(release_first_rx)),
            },
            call_count,
            first_started_rx,
            release_first_tx,
        )
    }
}

impl SpeechEngine for BlockingGateEngine {
    fn transcribe_short(
        &self,
        _request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError> {
        let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            if let Some(first_started_tx) = self.first_started_tx.lock().unwrap().take() {
                let _ = first_started_tx.send(());
            }
            if let Some(release_first_rx) = self.release_first_rx.lock().unwrap().take() {
                let _ = release_first_rx.recv();
            }
        }

        Ok(ShortAudioResult::new("ok", "blocking-gate"))
    }
}

fn sample_request() -> ShortAudioRequest {
    ShortAudioRequest {
        audio_bytes: vec![0_u8; 32],
        content_type: "audio/pcm".to_string(),
        engine: None,
        language_hint: None,
        timestamps: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn run_blocking_inference_uses_blocking_thread_pool() {
    let caller_thread_id = std::thread::current().id();
    let (engine, seen_thread_id) = ThreadRecordingEngine::new();

    let runtime = aximo::engine_runtime::EngineRuntime::new(Arc::new(engine));
    let result = aximo::inference_task::run_blocking_inference(runtime, sample_request())
        .await
        .unwrap();

    assert_eq!(result.text, "offloaded");
    assert_ne!(seen_thread_id.lock().unwrap().unwrap(), caller_thread_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timed_out_blocking_inference_holds_model_gate_until_backend_returns() {
    let (engine, call_count, first_started_rx, release_first_tx) = BlockingGateEngine::new();
    let runtime = aximo::engine_runtime::EngineRuntime::new(Arc::new(engine));

    let first = tokio::spawn(aximo::inference_task::run_blocking_inference_with_timeout(
        runtime.clone(),
        sample_request(),
        Duration::from_millis(10),
    ));

    first_started_rx.await.unwrap();
    assert!(matches!(
        first.await.unwrap(),
        Err(aximo::inference_task::BlockingInferenceError::Timeout { .. })
    ));

    let second = tokio::spawn(aximo::inference_task::run_blocking_inference_with_timeout(
        runtime,
        sample_request(),
        Duration::from_millis(20),
    ));
    tokio::time::sleep(Duration::from_millis(5)).await;

    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    release_first_tx.send(()).unwrap();
    assert!(second.await.unwrap().is_ok());
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeout_while_waiting_for_model_gate_records_wait_timeout_metric() {
    let (engine, _call_count, first_started_rx, release_first_tx) = BlockingGateEngine::new();
    let runtime = aximo::engine_runtime::EngineRuntime::new(Arc::new(engine));

    let first = tokio::spawn(aximo::inference_task::run_blocking_inference_with_timeout(
        runtime.clone(),
        sample_request(),
        Duration::from_millis(10),
    ));
    first_started_rx.await.unwrap();
    assert!(matches!(
        first.await.unwrap(),
        Err(aximo::inference_task::BlockingInferenceError::Timeout { .. })
    ));

    let metrics = aximo::metrics::Metrics::default();
    let second = aximo::inference_task::run_observed_blocking_inference_with_timeout(
        runtime,
        sample_request(),
        Duration::from_millis(10),
        metrics.clone(),
        "short",
    )
    .await;

    assert!(matches!(
        second,
        Err(aximo::inference_task::BlockingInferenceError::Timeout { .. })
    ));

    release_first_tx.send(()).unwrap();
    let rendered = metrics.render_prometheus();
    assert!(rendered.contains(r#"aximo_model_execution_wait_seconds_count{kind="short"} 1"#));
    assert!(rendered.contains(r#"aximo_model_execution_wait_timeouts_total{kind="short"} 1"#));
}
