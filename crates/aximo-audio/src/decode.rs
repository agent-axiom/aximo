use std::io::{Cursor, ErrorKind};

use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as SymphoniaError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};
use symphonia::default::{get_codecs, get_probe};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("unsupported content type {0}")]
    UnsupportedContentType(String),
    #[error("audio payload too large: {0}")]
    TooLarge(String),
    #[error("invalid raw pcm payload: {0}")]
    InvalidPcm(String),
    #[error("failed to decode audio container: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

pub fn decode_container(bytes: &[u8], content_type: &str) -> Result<DecodedAudio, AudioError> {
    decode_container_with_sample_limit(bytes, content_type, usize::MAX)
}

pub fn decode_container_with_sample_limit(
    bytes: &[u8],
    content_type: &str,
    max_decoded_samples: usize,
) -> Result<DecodedAudio, AudioError> {
    let mut hint = Hint::new();
    if let Some(extension) = extension_hint(content_type) {
        hint.with_extension(extension);
    }

    let media_source =
        MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let probed = get_probe()
        .format(
            &hint,
            media_source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| AudioError::Decode(error.to_string()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| AudioError::Decode("no default audio track found".to_string()))?;
    let track_id = track.id;
    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| AudioError::Decode(error.to_string()))?;
    let mut sample_rate = track.codec_params.sample_rate;
    let mut channels = track.codec_params.channels.map(|channels| channels.count());
    let mut samples = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err(AudioError::Decode(
                    "decoder reset is not supported for short-audio ingest".to_string(),
                ));
            }
            Err(error) => return Err(AudioError::Decode(error.to_string())),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => return Err(AudioError::Decode(error.to_string())),
        };

        sample_rate.get_or_insert(decoded.spec().rate);
        channels.get_or_insert(decoded.spec().channels.count());

        let mut sample_buffer =
            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        sample_buffer.copy_interleaved_ref(decoded);
        samples.extend_from_slice(sample_buffer.samples());
        if samples.len() > max_decoded_samples {
            return Err(AudioError::TooLarge(format!(
                "decoded audio exceeded {max_decoded_samples} samples"
            )));
        }
    }

    let sample_rate = sample_rate
        .ok_or_else(|| AudioError::Decode("decoded stream missing sample rate".to_string()))?;
    let channels = channels
        .ok_or_else(|| AudioError::Decode("decoded stream missing channel layout".to_string()))?;

    if samples.is_empty() {
        return Err(AudioError::Decode(
            "decoded audio stream did not contain samples".to_string(),
        ));
    }

    Ok(DecodedAudio {
        sample_rate,
        channels,
        samples,
    })
}

fn extension_hint(content_type: &str) -> Option<&'static str> {
    if content_type.contains("wav") {
        Some("wav")
    } else if content_type.contains("mpeg") || content_type.contains("mp3") {
        Some("mp3")
    } else if content_type.contains("flac") {
        Some("flac")
    } else if content_type.contains("mp4")
        || content_type.contains("m4a")
        || content_type.contains("aac")
    {
        Some("m4a")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        std::fs::read(fixtures_dir.join(name)).unwrap()
    }

    #[test]
    fn extension_hint_maps_supported_content_types() {
        assert_eq!(extension_hint("audio/wav"), Some("wav"));
        assert_eq!(extension_hint("audio/mpeg"), Some("mp3"));
        assert_eq!(extension_hint("audio/flac"), Some("flac"));
        assert_eq!(extension_hint("audio/mp4"), Some("m4a"));
        assert_eq!(extension_hint("application/json"), None);
    }

    #[test]
    fn decode_container_reads_wav_fixture_samples() {
        let decoded = decode_container(&fixture_bytes("tone-16k-mono.wav"), "audio/wav").unwrap();

        assert_eq!(decoded.sample_rate, 16_000);
        assert_eq!(decoded.channels, 1);
        assert!(!decoded.samples.is_empty());
    }

    #[test]
    fn decode_container_rejects_invalid_bytes() {
        let error = decode_container(b"not-audio", "audio/mpeg").unwrap_err();
        assert!(matches!(error, AudioError::Decode(_)));
    }

    #[test]
    fn decode_container_rejects_streams_over_sample_limit() {
        let error = decode_container_with_sample_limit(
            &fixture_bytes("tone-16k-mono.mp3"),
            "audio/mpeg",
            1,
        )
        .unwrap_err();

        assert!(matches!(error, AudioError::TooLarge(_)));
    }
}
