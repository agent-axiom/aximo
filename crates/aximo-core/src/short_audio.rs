use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortAudioRequest {
    pub audio_bytes: Vec<u8>,
    pub content_type: String,
    pub engine: Option<String>,
    pub language_hint: Option<String>,
    pub timestamps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineCapabilities {
    /// Stable engine identifier used in API requests and configuration.
    pub engine: String,
    /// Human-readable model/backend name reported by the adapter.
    pub model_name: String,
    /// Expected model input sample rate in Hz.
    pub sample_rate_hz: u32,
    /// BCP-47 language codes reported by the backend. Empty means unknown.
    pub languages: Vec<String>,
    /// Whether the backend can return segment/timestamp metadata.
    pub supports_timestamps: bool,
    /// Whether the backend exposes detected-language output.
    pub supports_language_detection: bool,
    /// Whether the backend performs native incremental streaming inference.
    pub supports_native_streaming: bool,
}

impl EngineCapabilities {
    pub fn unknown(engine: impl Into<String>) -> Self {
        let engine = engine.into();
        Self {
            model_name: engine.clone(),
            engine,
            sample_rate_hz: 16_000,
            languages: Vec::new(),
            supports_timestamps: false,
            supports_language_detection: false,
            supports_native_streaming: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShortAudioResult {
    /// Full transcript text returned by the backend.
    pub text: String,
    /// Segment metadata when the active backend exposes structured timestamps.
    /// Empty when the current integration only returns plain transcript text.
    pub segments: Vec<TranscriptSegment>,
    /// Detected language when the backend exposes it.
    /// `None` when language detection is unavailable from the current adapter.
    pub detected_language: Option<String>,
    /// Engine identifier that produced the result.
    pub engine: String,
    /// Measured input audio duration in milliseconds.
    pub duration_ms: u64,
    /// Measured processing time in milliseconds.
    pub processing_ms: u64,
}

impl ShortAudioResult {
    /// Convenience constructor for text-only backends and test doubles.
    ///
    /// This intentionally leaves optional STT metadata empty instead of inventing
    /// segments or language detection that the backend did not provide.
    pub fn new(text: impl Into<String>, engine: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            segments: Vec::new(),
            detected_language: None,
            engine: engine.into(),
            duration_ms: 0,
            processing_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}
