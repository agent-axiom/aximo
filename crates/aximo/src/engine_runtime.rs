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
