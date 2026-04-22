use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

#[derive(Clone)]
pub struct Scheduler {
    short_audio_requests: Arc<Semaphore>,
    realtime_sessions: Arc<Semaphore>,
    short_inferences: Arc<Semaphore>,
    realtime_inferences: Arc<Semaphore>,
}

impl Scheduler {
    pub fn new(
        max_short_audio_requests: usize,
        max_realtime_sessions: usize,
        max_short_inferences: usize,
        max_realtime_inferences: usize,
    ) -> Self {
        Self {
            short_audio_requests: Arc::new(Semaphore::new(max_short_audio_requests.max(1))),
            realtime_sessions: Arc::new(Semaphore::new(max_realtime_sessions.max(1))),
            short_inferences: Arc::new(Semaphore::new(max_short_inferences.max(1))),
            realtime_inferences: Arc::new(Semaphore::new(max_realtime_inferences.max(1))),
        }
    }

    pub fn try_acquire_short_audio_request(&self) -> Result<OwnedSemaphorePermit, CapacityError> {
        self.short_audio_requests
            .clone()
            .try_acquire_owned()
            .map_err(map_capacity_error)
    }

    pub fn try_acquire_realtime_session(&self) -> Result<OwnedSemaphorePermit, CapacityError> {
        self.realtime_sessions
            .clone()
            .try_acquire_owned()
            .map_err(map_capacity_error)
    }

    pub fn try_acquire_short_inference(&self) -> Result<OwnedSemaphorePermit, CapacityError> {
        self.short_inferences
            .clone()
            .try_acquire_owned()
            .map_err(map_capacity_error)
    }

    pub fn try_acquire_realtime_inference(&self) -> Result<OwnedSemaphorePermit, CapacityError> {
        self.realtime_inferences
            .clone()
            .try_acquire_owned()
            .map_err(map_capacity_error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapacityError {
    Saturated,
}

fn map_capacity_error(_: TryAcquireError) -> CapacityError {
    CapacityError::Saturated
}
