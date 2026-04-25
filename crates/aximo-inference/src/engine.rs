use aximo_core::{EngineCapabilities, ShortAudioRequest, ShortAudioResult};
use thiserror::Error;

pub trait StreamingSpeechSession: Send {
    fn accept_pcm_chunk(
        &mut self,
        chunk: &[u8],
    ) -> Result<Option<ShortAudioResult>, InferenceError>;

    fn finish(&mut self) -> Result<ShortAudioResult, InferenceError>;
}

pub trait SpeechEngine: Send + Sync {
    fn transcribe_short(
        &self,
        request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError>;

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::unknown("unknown")
    }

    fn start_streaming_session(&self) -> Result<Box<dyn StreamingSpeechSession>, InferenceError> {
        Err(InferenceError::UnsupportedStreaming(
            self.capabilities().engine,
        ))
    }
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
    #[error("native streaming is not supported by engine: {0}")]
    UnsupportedStreaming(String),
}

pub struct FakeEngine;

impl SpeechEngine for FakeEngine {
    fn transcribe_short(
        &self,
        _request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError> {
        Ok(ShortAudioResult::new("hello world", "fake"))
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            engine: "fake".to_string(),
            model_name: "FakeEngine".to_string(),
            sample_rate_hz: 16_000,
            languages: vec!["en".to_string(), "ru".to_string()],
            supports_timestamps: true,
            supports_language_detection: false,
            supports_native_streaming: false,
        }
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

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::unknown("unavailable")
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
    fn fake_engine_reports_test_capabilities() {
        let capabilities = FakeEngine.capabilities();

        assert_eq!(capabilities.engine, "fake");
        assert_eq!(
            capabilities.languages,
            vec!["en".to_string(), "ru".to_string()]
        );
        assert!(capabilities.supports_timestamps);
        assert!(!capabilities.supports_language_detection);
        assert!(!capabilities.supports_native_streaming);
    }

    #[test]
    fn default_streaming_session_reports_unsupported() {
        match FakeEngine.start_streaming_session() {
            Err(InferenceError::UnsupportedStreaming(engine)) => assert_eq!(engine, "fake"),
            Err(error) => panic!("unexpected error: {error}"),
            Ok(_) => panic!("fake engine should not start a streaming session"),
        }
    }

    #[test]
    fn unavailable_engine_returns_unavailable_error() {
        let error = UnavailableEngine::new("missing model")
            .transcribe_short(sample_request())
            .unwrap_err();

        assert!(matches!(error, InferenceError::Unavailable(_)));
    }
}
