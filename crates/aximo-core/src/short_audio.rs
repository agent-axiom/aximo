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
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    pub detected_language: Option<String>,
    pub engine: String,
    pub duration_ms: u64,
    pub processing_ms: u64,
}

impl ShortAudioResult {
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
