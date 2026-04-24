use std::time::Duration;

use aximo_core::{ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::InferenceError;
use thiserror::Error;

use crate::engine_runtime::EngineRuntime;

#[derive(Debug, Error)]
pub enum BlockingInferenceError {
    #[error("inference timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error(transparent)]
    Inference(#[from] InferenceError),
}

pub async fn run_blocking_inference(
    runtime: EngineRuntime,
    request: ShortAudioRequest,
) -> Result<ShortAudioResult, InferenceError> {
    let permit = runtime.acquire_execution_permit().await;
    let engine = runtime.engine();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        engine.transcribe_short(request)
    })
    .await
    .map_err(|error| InferenceError::Runtime(format!("blocking inference task failed: {error}")))?
}

pub async fn run_blocking_inference_with_timeout(
    runtime: EngineRuntime,
    request: ShortAudioRequest,
    timeout_duration: Duration,
) -> Result<ShortAudioResult, BlockingInferenceError> {
    let timeout_ms = timeout_duration.as_millis().try_into().unwrap_or(u64::MAX);
    let task = async move {
        let permit = runtime.acquire_execution_permit().await;
        let engine = runtime.engine();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            engine.transcribe_short(request)
        })
        .await
        .map_err(|error| {
            InferenceError::Runtime(format!("blocking inference task failed: {error}"))
        })?
        .map_err(BlockingInferenceError::Inference)
    };

    match tokio::time::timeout(timeout_duration, task).await {
        Ok(result) => result,
        Err(_) => Err(BlockingInferenceError::Timeout { timeout_ms }),
    }
}
