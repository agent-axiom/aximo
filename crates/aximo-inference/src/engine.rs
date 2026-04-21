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
    #[error("unsupported engine: {0}")]
    UnsupportedEngine(String),
    #[error("invalid audio payload: {0}")]
    InvalidAudio(String),
    #[error("runtime inference error: {0}")]
    Runtime(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> ShortAudioRequest {
        ShortAudioRequest {
            audio_bytes: vec![0, 1, 2, 3],
            content_type: "audio/wav".to_string(),
            engine: None,
            language_hint: None,
            timestamps: false,
        }
    }

    #[test]
    fn fake_engine_returns_static_result() {
        let result = FakeEngine.transcribe_short(sample_request()).unwrap();

        assert_eq!(result.text, "hello world");
        assert_eq!(result.engine, "fake");
    }

    #[test]
    fn unavailable_engine_returns_unavailable_error() {
        let error = UnavailableEngine::new("missing model")
            .transcribe_short(sample_request())
            .unwrap_err();

        assert!(matches!(error, InferenceError::Unavailable(_)));
    }
}
