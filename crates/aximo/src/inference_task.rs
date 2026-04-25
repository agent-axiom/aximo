use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use aximo_core::{ShortAudioRequest, ShortAudioResult};
use aximo_inference::engine::InferenceError;
use thiserror::Error;

use crate::{engine_runtime::EngineRuntime, metrics::Metrics};

#[derive(Debug, Error)]
pub enum BlockingInferenceError {
    #[error("inference timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error(transparent)]
    Inference(#[from] InferenceError),
}

#[derive(Clone)]
struct BlockingInferenceObserver {
    metrics: Metrics,
    kind: &'static str,
}

struct ActiveBlockingInferenceGuard {
    metrics: Metrics,
}

impl ActiveBlockingInferenceGuard {
    fn new(observer: &BlockingInferenceObserver) -> Self {
        observer.metrics.inc_blocking_tasks_active();
        observer.metrics.inc_model_executions_active();
        Self {
            metrics: observer.metrics.clone(),
        }
    }
}

impl Drop for ActiveBlockingInferenceGuard {
    fn drop(&mut self) {
        self.metrics.dec_model_executions_active();
        self.metrics.dec_blocking_tasks_active();
    }
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
    run_blocking_inference_with_timeout_inner(runtime, request, timeout_duration, None).await
}

pub async fn run_observed_blocking_inference_with_timeout(
    runtime: EngineRuntime,
    request: ShortAudioRequest,
    timeout_duration: Duration,
    metrics: Metrics,
    kind: &'static str,
) -> Result<ShortAudioResult, BlockingInferenceError> {
    run_blocking_inference_with_timeout_inner(
        runtime,
        request,
        timeout_duration,
        Some(BlockingInferenceObserver { metrics, kind }),
    )
    .await
}

async fn run_blocking_inference_with_timeout_inner(
    runtime: EngineRuntime,
    request: ShortAudioRequest,
    timeout_duration: Duration,
    observer: Option<BlockingInferenceObserver>,
) -> Result<ShortAudioResult, BlockingInferenceError> {
    let timeout_ms = timeout_duration.as_millis().try_into().unwrap_or(u64::MAX);
    let timeout_observer = observer.clone();
    let model_wait_started_at = std::time::Instant::now();
    let execution_permit_acquired = Arc::new(AtomicBool::new(false));
    let timeout_execution_permit_acquired = Arc::clone(&execution_permit_acquired);
    let task = async move {
        let permit = runtime.acquire_execution_permit().await;
        execution_permit_acquired.store(true, Ordering::SeqCst);
        if let Some(observer) = &observer {
            observer
                .metrics
                .record_model_execution_wait(observer.kind, model_wait_started_at.elapsed());
        }
        let engine = runtime.engine();
        let active_observer = observer.clone();
        tokio::task::spawn_blocking(move || {
            let _active_guard = active_observer
                .as_ref()
                .map(ActiveBlockingInferenceGuard::new);
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
        Err(_) => {
            if let Some(observer) = timeout_observer {
                if !timeout_execution_permit_acquired.load(Ordering::SeqCst) {
                    observer.metrics.record_model_execution_wait(
                        observer.kind,
                        model_wait_started_at.elapsed(),
                    );
                    observer
                        .metrics
                        .record_model_execution_wait_timeout(observer.kind);
                }
                observer.metrics.record_inference_timeout(observer.kind);
            }
            Err(BlockingInferenceError::Timeout { timeout_ms })
        }
    }
}
