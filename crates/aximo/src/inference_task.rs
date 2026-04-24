use std::{sync::Arc, time::Duration};

use aximo_core::{ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::{InferenceError, SpeechEngine};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BlockingInferenceError {
    #[error("inference timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error(transparent)]
    Inference(#[from] InferenceError),
}

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

pub async fn run_blocking_inference_with_timeout(
    engine: Arc<dyn SpeechEngine>,
    request: ShortAudioRequest,
    timeout_duration: Duration,
) -> Result<ShortAudioResult, BlockingInferenceError> {
    let timeout_ms = timeout_duration.as_millis().try_into().unwrap_or(u64::MAX);
    let task = tokio::task::spawn_blocking(move || engine.transcribe_short(request));
    match tokio::time::timeout(timeout_duration, task).await {
        Ok(result) => result
            .map_err(|error| {
                InferenceError::Runtime(format!("blocking inference task failed: {error}"))
            })?
            .map_err(BlockingInferenceError::Inference),
        Err(_) => Err(BlockingInferenceError::Timeout { timeout_ms }),
    }
}
