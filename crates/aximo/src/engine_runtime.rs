use std::sync::Arc;

use aximo_core::EngineCapabilities;
use aximo_inference::engine::{InferenceError, SpeechEngine, StreamingSpeechSession};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub struct EngineRuntime {
    engine: Arc<dyn SpeechEngine>,
    execution_gate: Arc<Semaphore>,
}

impl EngineRuntime {
    pub fn new(engine: Arc<dyn SpeechEngine>) -> Self {
        Self::with_gate(engine, Arc::new(Semaphore::new(1)))
    }

    pub fn with_gate(engine: Arc<dyn SpeechEngine>, execution_gate: Arc<Semaphore>) -> Self {
        Self {
            engine,
            execution_gate,
        }
    }

    pub fn shared_gate() -> Arc<Semaphore> {
        Arc::new(Semaphore::new(1))
    }

    pub fn engine(&self) -> Arc<dyn SpeechEngine> {
        Arc::clone(&self.engine)
    }

    pub fn capabilities(&self) -> EngineCapabilities {
        self.engine.capabilities()
    }

    pub fn start_streaming_session(
        &self,
    ) -> Result<Box<dyn StreamingSpeechSession>, InferenceError> {
        self.engine.start_streaming_session()
    }

    pub async fn acquire_execution_permit(&self) -> OwnedSemaphorePermit {
        self.execution_gate
            .clone()
            .acquire_owned()
            .await
            .expect("engine execution gate closed")
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use aximo_inference::engine::{FakeEngine, InferenceError};

    use super::*;

    #[test]
    fn runtime_forwards_engine_capabilities_and_streaming_start() {
        let runtime = EngineRuntime::new(Arc::new(FakeEngine));

        let capabilities = runtime.capabilities();
        assert_eq!(capabilities.engine, "fake");
        assert_eq!(capabilities.sample_rate_hz, 16_000);

        let error = match runtime.start_streaming_session() {
            Ok(_) => panic!("fake engine should not start a streaming session"),
            Err(error) => error,
        };
        assert!(matches!(error, InferenceError::UnsupportedStreaming(_)));
    }

    #[tokio::test]
    async fn shared_gate_serializes_execution_permits() {
        let gate = EngineRuntime::shared_gate();
        let first_runtime = EngineRuntime::with_gate(Arc::new(FakeEngine), Arc::clone(&gate));
        let second_runtime = EngineRuntime::with_gate(Arc::new(FakeEngine), gate);

        let first_permit = first_runtime.acquire_execution_permit().await;
        assert!(tokio::time::timeout(
            Duration::from_millis(10),
            second_runtime.acquire_execution_permit()
        )
        .await
        .is_err());

        drop(first_permit);
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            second_runtime.acquire_execution_permit()
        )
        .await
        .is_ok());
    }
}
