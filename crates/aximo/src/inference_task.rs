use std::sync::Arc;

use aximo_core::{ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::{InferenceError, SpeechEngine};

pub async fn run_blocking_inference(
    engine: Arc<dyn SpeechEngine>,
    request: ShortAudioRequest,
) -> Result<ShortAudioResult, InferenceError> {
    tokio::task::spawn_blocking(move || engine.transcribe_short(request))
        .await
        .map_err(|error| {
            InferenceError::Runtime(format!("blocking inference task failed: {error}"))
        })?
}
