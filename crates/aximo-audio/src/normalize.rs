use std::io::Cursor;

use hound::{SampleFormat, WavReader};

use crate::{decode_container_with_sample_limit, AudioError, DecodedAudio};

const TARGET_SAMPLE_RATE: u32 = 16_000;
const PCM_BYTES_PER_SAMPLE: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAudio {
    pub audio_bytes: Vec<u8>,
    pub content_type: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortAudioLimits {
    pub max_raw_pcm_bytes: usize,
    pub max_duration_ms: u64,
    pub max_decoded_samples: usize,
}

impl ShortAudioLimits {
    pub const fn unbounded() -> Self {
        Self {
            max_raw_pcm_bytes: usize::MAX,
            max_duration_ms: u64::MAX,
            max_decoded_samples: usize::MAX,
        }
    }
}

pub fn prepare_short_audio(bytes: &[u8], content_type: &str) -> Result<PreparedAudio, AudioError> {
    prepare_short_audio_with_limits(bytes, content_type, ShortAudioLimits::unbounded())
}

pub fn prepare_short_audio_with_limits(
    bytes: &[u8],
    content_type: &str,
    limits: ShortAudioLimits,
) -> Result<PreparedAudio, AudioError> {
    if content_type.contains("pcm") || content_type.contains("octet-stream") {
        validate_raw_pcm(bytes, limits)?;
        return Ok(PreparedAudio {
            audio_bytes: bytes.to_vec(),
            content_type: "audio/pcm",
        });
    }

    if content_type.contains("wav") {
        let wav_info = wav_info(bytes)?;
        if wav_info.is_passthrough {
            validate_duration_ms(wav_info.duration_ms, limits)?;
            return Ok(PreparedAudio {
                audio_bytes: bytes.to_vec(),
                content_type: "audio/wav",
            });
        }
    } else if !is_supported_container(content_type) {
        return Err(AudioError::UnsupportedContentType(content_type.to_string()));
    }

    let decoded = decode_container_with_sample_limit(
        bytes,
        content_type,
        limits.max_decoded_samples,
    )?;
    validate_duration_ms(decoded_duration_ms(&decoded), limits)?;
    let audio_bytes = normalize_decoded_audio(decoded);

    Ok(PreparedAudio {
        audio_bytes,
        content_type: "audio/pcm",
    })
}

fn validate_raw_pcm(bytes: &[u8], limits: ShortAudioLimits) -> Result<(), AudioError> {
    if bytes.len() % PCM_BYTES_PER_SAMPLE != 0 {
        return Err(AudioError::InvalidPcm(
            "pcm payload must be aligned to 16-bit samples".to_string(),
        ));
    }

    if bytes.len() > limits.max_raw_pcm_bytes {
        return Err(AudioError::TooLarge(format!(
            "raw pcm payload exceeded {} bytes",
            limits.max_raw_pcm_bytes
        )));
    }

    let sample_count = bytes.len() / PCM_BYTES_PER_SAMPLE;
    let duration_ms = sample_count as u64 * 1000 / u64::from(TARGET_SAMPLE_RATE);
    validate_duration_ms(duration_ms, limits)?;

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct WavInfo {
    is_passthrough: bool,
    duration_ms: u64,
}

fn wav_info(bytes: &[u8]) -> Result<WavInfo, AudioError> {
    let reader = WavReader::new(Cursor::new(bytes))
        .map_err(|error| AudioError::Decode(error.to_string()))?;
    let spec = reader.spec();
    let sample_rate = u64::from(spec.sample_rate);
    let channels = u64::from(spec.channels);
    if sample_rate == 0 || channels == 0 {
        return Err(AudioError::Decode(
            "wav metadata must declare non-zero sample rate and channels".to_string(),
        ));
    }
    let frames = u64::from(reader.duration()) / channels;

    Ok(WavInfo {
        is_passthrough: spec.channels == 1
            && spec.sample_rate == TARGET_SAMPLE_RATE
            && spec.bits_per_sample == 16
            && spec.sample_format == SampleFormat::Int,
        duration_ms: frames.saturating_mul(1000) / sample_rate,
    })
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

fn decoded_duration_ms(decoded: &DecodedAudio) -> u64 {
    if decoded.sample_rate == 0 || decoded.channels == 0 {
        return 0;
    }

    let frames = decoded.samples.len() / decoded.channels;
    frames as u64 * 1000 / u64::from(decoded.sample_rate)
}

fn validate_duration_ms(duration_ms: u64, limits: ShortAudioLimits) -> Result<(), AudioError> {
    if duration_ms > limits.max_duration_ms {
        return Err(AudioError::TooLarge(format!(
            "audio duration {duration_ms}ms exceeded {}ms",
            limits.max_duration_ms
        )));
    }

    Ok(())
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
    fn raw_pcm_is_rejected_when_over_byte_limit() {
        let error = prepare_short_audio_with_limits(
            &[0_u8; 8],
            "audio/pcm",
            ShortAudioLimits {
                max_raw_pcm_bytes: 4,
                max_duration_ms: u64::MAX,
                max_decoded_samples: usize::MAX,
            },
        )
        .unwrap_err();

        assert!(matches!(error, AudioError::TooLarge(_)));
    }

    #[test]
    fn wav_is_rejected_when_over_duration_limit() {
        let error = prepare_short_audio_with_limits(
            &fixture_bytes("tone-16k-mono.wav"),
            "audio/wav",
            ShortAudioLimits {
                max_raw_pcm_bytes: usize::MAX,
                max_duration_ms: 1,
                max_decoded_samples: usize::MAX,
            },
        )
        .unwrap_err();

        assert!(matches!(error, AudioError::TooLarge(_)));
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
