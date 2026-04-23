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
