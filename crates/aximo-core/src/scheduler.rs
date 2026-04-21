use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

#[derive(Clone)]
pub struct Scheduler {
    short_audio: Arc<Semaphore>,
    realtime: Arc<Semaphore>,
}

impl Scheduler {
    pub fn new(max_short_audio: usize, max_realtime: usize) -> Self {
        Self {
            short_audio: Arc::new(Semaphore::new(max_short_audio.max(1))),
            realtime: Arc::new(Semaphore::new(max_realtime.max(1))),
        }
    }

    pub fn try_acquire_short_audio(&self) -> Result<OwnedSemaphorePermit, CapacityError> {
        self.short_audio
            .clone()
            .try_acquire_owned()
            .map_err(map_capacity_error)
    }

    pub fn try_acquire_realtime(&self) -> Result<OwnedSemaphorePermit, CapacityError> {
        self.realtime
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
