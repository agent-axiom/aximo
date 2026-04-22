use std::{
    sync::{Arc, Mutex},
    thread::ThreadId,
};

use aximo_core::{ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::{InferenceError, SpeechEngine};

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

    let result = aximo::inference_task::run_blocking_inference(Arc::new(engine), sample_request())
        .await
        .unwrap();

    assert_eq!(result.text, "offloaded");
    assert_ne!(seen_thread_id.lock().unwrap().unwrap(), caller_thread_id);
}
