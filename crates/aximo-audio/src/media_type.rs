use crate::AudioError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMediaType {
    Wav,
    Mp3,
    Flac,
    Mp4,
    RawPcm,
}

impl AudioMediaType {
    pub const fn canonical_content_type(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::RawPcm => "audio/pcm",
            Self::Mp3 => "audio/mpeg",
            Self::Flac => "audio/flac",
            Self::Mp4 => "audio/mp4",
        }
    }

    pub const fn extension_hint(self) -> Option<&'static str> {
        match self {
            Self::Wav => Some("wav"),
            Self::Mp3 => Some("mp3"),
            Self::Flac => Some("flac"),
            Self::Mp4 => Some("m4a"),
            Self::RawPcm => None,
        }
    }
}

pub fn parse_audio_media_type(content_type: &str) -> Result<AudioMediaType, AudioError> {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if media_type.is_empty() {
        return Err(AudioError::UnsupportedContentType(
            "missing content type".to_string(),
        ));
    }

    match media_type.as_str() {
        "audio/wav" | "audio/wave" | "audio/x-wav" | "audio/vnd.wave" => {
            Ok(AudioMediaType::Wav)
        }
        "audio/mpeg" | "audio/mp3" => Ok(AudioMediaType::Mp3),
        "audio/flac" | "audio/x-flac" => Ok(AudioMediaType::Flac),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" | "audio/aac" | "audio/aacp" => {
            Ok(AudioMediaType::Mp4)
        }
        "audio/pcm" | "audio/l16" | "application/octet-stream" => Ok(AudioMediaType::RawPcm),
        _ => Err(AudioError::UnsupportedContentType(media_type)),
    }
}
