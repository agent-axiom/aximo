use aximo_core::{ShortAudioRequest, ShortAudioResult};
use thiserror::Error;

pub trait SpeechEngine: Send + Sync {
    fn transcribe_short(
        &self,
        request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError>;
}

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("speech engine unavailable: {0}")]
    Unavailable(String),
}

pub struct FakeEngine;

impl SpeechEngine for FakeEngine {
    fn transcribe_short(
        &self,
        _request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError> {
        Ok(ShortAudioResult::new("hello world", "fake"))
    }
}

pub struct UnavailableEngine {
    reason: String,
}

impl UnavailableEngine {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl SpeechEngine for UnavailableEngine {
    fn transcribe_short(
        &self,
        _request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError> {
        Err(InferenceError::Unavailable(self.reason.clone()))
    }
}
