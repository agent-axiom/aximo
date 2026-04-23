use std::io::Cursor;

use hound::{SampleFormat, WavReader};

use crate::{decode_container, AudioError, DecodedAudio};

const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAudio {
    pub audio_bytes: Vec<u8>,
    pub content_type: &'static str,
}

pub fn prepare_short_audio(bytes: &[u8], content_type: &str) -> Result<PreparedAudio, AudioError> {
    if content_type.contains("pcm") || content_type.contains("octet-stream") {
        validate_raw_pcm(bytes)?;
        return Ok(PreparedAudio {
            audio_bytes: bytes.to_vec(),
            content_type: "audio/pcm",
        });
    }

    if content_type.contains("wav") {
        if is_passthrough_wav(bytes)? {
            return Ok(PreparedAudio {
                audio_bytes: bytes.to_vec(),
                content_type: "audio/wav",
            });
        }
    } else if !is_supported_container(content_type) {
        return Err(AudioError::UnsupportedContentType(content_type.to_string()));
    }

    let decoded = decode_container(bytes, content_type)?;
    let audio_bytes = normalize_decoded_audio(decoded);

    Ok(PreparedAudio {
        audio_bytes,
        content_type: "audio/pcm",
    })
}

fn validate_raw_pcm(bytes: &[u8]) -> Result<(), AudioError> {
    if bytes.len() % 2 != 0 {
        return Err(AudioError::InvalidPcm(
            "pcm payload must be aligned to 16-bit samples".to_string(),
        ));
    }

    Ok(())
}

fn is_passthrough_wav(bytes: &[u8]) -> Result<bool, AudioError> {
    let reader = WavReader::new(Cursor::new(bytes))
        .map_err(|error| AudioError::Decode(error.to_string()))?;
    let spec = reader.spec();

    Ok(spec.channels == 1
        && spec.sample_rate == TARGET_SAMPLE_RATE
        && spec.bits_per_sample == 16
        && spec.sample_format == SampleFormat::Int)
}

fn is_supported_container(content_type: &str) -> bool {
    content_type.contains("wav")
        || content_type.contains("mpeg")
        || content_type.contains("mp3")
        || content_type.contains("flac")
        || content_type.contains("mp4")
        || content_type.contains("m4a")
        || content_type.contains("aac")
}

fn normalize_decoded_audio(decoded: DecodedAudio) -> Vec<u8> {
    let mono = downmix_to_mono(&decoded.samples, decoded.channels);
    let resampled = if decoded.sample_rate == TARGET_SAMPLE_RATE {
        mono
    } else {
        linear_resample(&mono, decoded.sample_rate, TARGET_SAMPLE_RATE)
    };

    encode_pcm_s16le(&resampled)
}

fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }

    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect()
}

fn linear_resample(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if samples.is_empty() || source_rate == 0 || target_rate == 0 {
        return Vec::new();
    }

    if source_rate == target_rate {
        return samples.to_vec();
    }

    let output_len =
        ((samples.len() as u64 * target_rate as u64) / source_rate as u64).max(1) as usize;
    let mut output = Vec::with_capacity(output_len);

    for index in 0..output_len {
        let position = index as f64 * source_rate as f64 / target_rate as f64;
        let left_index = position.floor() as usize;
        let right_index = (left_index + 1).min(samples.len().saturating_sub(1));
        let weight = (position - left_index as f64) as f32;
        let left = samples[left_index];
        let right = samples[right_index];
        output.push(left + (right - left) * weight);
    }

    output
}

fn encode_pcm_s16le(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);

    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let quantized = (clamped * i16::MAX as f32).round() as i16;
        bytes.extend_from_slice(&quantized.to_le_bytes());
    }

    bytes
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
    fn raw_pcm_is_passed_through() {
        let bytes = vec![0_u8, 0, 1, 0];

        let prepared = prepare_short_audio(&bytes, "audio/pcm").unwrap();

        assert_eq!(prepared.content_type, "audio/pcm");
        assert_eq!(prepared.audio_bytes, bytes);
    }

    #[test]
    fn stereo_wav_is_decoded_and_resampled_to_pcm() {
        let wav = fixture_bytes("tone-44k-stereo.wav");

        let prepared = prepare_short_audio(&wav, "audio/wav").unwrap();

        assert_eq!(prepared.content_type, "audio/pcm");
        assert_eq!(prepared.audio_bytes.len() % 2, 0);
        assert!(!prepared.audio_bytes.is_empty());
    }

    #[test]
    fn downmix_to_mono_averages_channels() {
        let mono = downmix_to_mono(&[1.0, -1.0, 0.5, 0.5], 2);
        assert_eq!(mono, vec![0.0, 0.5]);
    }

    #[test]
    fn linear_resample_changes_sample_count_for_new_rate() {
        let resampled = linear_resample(&[0.0, 0.5, 1.0, 0.5], 8_000, 16_000);
        assert!(resampled.len() > 4);
    }

    #[test]
    fn encode_pcm_s16le_clamps_samples() {
        let encoded = encode_pcm_s16le(&[-2.0, 0.0, 2.0]);
        let decoded: Vec<i16> = encoded
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        assert_eq!(decoded[0], i16::MIN + 1);
        assert_eq!(decoded[1], 0);
        assert_eq!(decoded[2], i16::MAX);
    }
}
