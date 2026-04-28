use std::io::Cursor;

use bytes::Bytes;
use hound::{SampleFormat, WavReader};

use crate::{
    decode_container_bytes_with_sample_limit, parse_audio_media_type, AudioError, AudioMediaType,
    DecodedAudio,
};

const TARGET_SAMPLE_RATE: u32 = 16_000;
const PCM_BYTES_PER_SAMPLE: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAudio {
    pub audio_bytes: Vec<u8>,
    pub content_type: &'static str,
    pub duration_ms: u64,
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
    prepare_short_audio_bytes_with_limits(Bytes::copy_from_slice(bytes), content_type, limits)
}

pub fn prepare_short_audio_bytes_with_limits(
    bytes: Bytes,
    content_type: &str,
    limits: ShortAudioLimits,
) -> Result<PreparedAudio, AudioError> {
    let media_type = parse_audio_media_type(content_type)?;

    match media_type {
        AudioMediaType::RawPcm => {
            validate_raw_pcm(bytes.as_ref(), limits)?;
            return Ok(PreparedAudio {
                audio_bytes: bytes.to_vec(),
                content_type: AudioMediaType::RawPcm.canonical_content_type(),
                duration_ms: raw_pcm_duration_ms(bytes.as_ref()),
            });
        }
        AudioMediaType::Wav => {
            let wav_info = wav_info(bytes.as_ref())?;
            if wav_info.is_passthrough {
                validate_duration_ms(wav_info.duration_ms, limits)?;
                return Ok(PreparedAudio {
                    audio_bytes: bytes.to_vec(),
                    content_type: AudioMediaType::Wav.canonical_content_type(),
                    duration_ms: wav_info.duration_ms,
                });
            }
        }
        AudioMediaType::Mp3 | AudioMediaType::Flac | AudioMediaType::Mp4 => {}
    }

    let decoded =
        decode_container_bytes_with_sample_limit(bytes, content_type, limits.max_decoded_samples)?;
    let duration_ms = decoded_duration_ms(&decoded);
    validate_duration_ms(duration_ms, limits)?;
    let audio_bytes = normalize_decoded_audio(decoded);

    Ok(PreparedAudio {
        audio_bytes,
        content_type: "audio/pcm",
        duration_ms,
    })
}

fn validate_raw_pcm(bytes: &[u8], limits: ShortAudioLimits) -> Result<(), AudioError> {
    if !bytes.len().is_multiple_of(PCM_BYTES_PER_SAMPLE) {
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

    validate_duration_ms(raw_pcm_duration_ms(bytes), limits)?;

    Ok(())
}

fn raw_pcm_duration_ms(bytes: &[u8]) -> u64 {
    let sample_count = bytes.len() / PCM_BYTES_PER_SAMPLE;
    sample_count as u64 * 1000 / u64::from(TARGET_SAMPLE_RATE)
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

fn normalize_decoded_audio(decoded: DecodedAudio) -> Vec<u8> {
    let mono = downmix_to_mono(&decoded.samples, decoded.channels);
    let resampled = if decoded.sample_rate == TARGET_SAMPLE_RATE {
        mono
    } else {
        windowed_sinc_resample(&mono, decoded.sample_rate, TARGET_SAMPLE_RATE)
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

fn windowed_sinc_resample(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if samples.is_empty() || source_rate == 0 || target_rate == 0 {
        return Vec::new();
    }

    if source_rate == target_rate {
        return samples.to_vec();
    }

    let output_len =
        ((samples.len() as u64 * target_rate as u64) / source_rate as u64).max(1) as usize;
    let mut output = Vec::with_capacity(output_len);
    let cutoff = (target_rate as f64 / source_rate as f64).min(1.0);
    let radius = 8_i64;

    for index in 0..output_len {
        let position = index as f64 * source_rate as f64 / target_rate as f64;
        let center = position.floor() as i64;
        let mut weighted_sum = 0.0_f64;
        let mut weight_sum = 0.0_f64;

        for sample_index in (center - radius + 1)..=(center + radius) {
            if sample_index < 0 || sample_index >= samples.len() as i64 {
                continue;
            }

            let distance = position - sample_index as f64;
            let window = hann_window(distance, radius as f64);
            if window == 0.0 {
                continue;
            }
            let weight = cutoff * sinc(distance * cutoff) * window;
            weighted_sum += samples[sample_index as usize] as f64 * weight;
            weight_sum += weight;
        }

        let sample = if weight_sum.abs() > f64::EPSILON {
            weighted_sum / weight_sum
        } else {
            0.0
        };
        output.push(sample.clamp(-1.0, 1.0) as f32);
    }

    output
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-8 {
        return 1.0;
    }

    let pix = std::f64::consts::PI * x;
    pix.sin() / pix
}

fn hann_window(distance: f64, radius: f64) -> f64 {
    let normalized = distance.abs() / radius;
    if normalized >= 1.0 {
        return 0.0;
    }

    0.5 * (1.0 + (std::f64::consts::PI * normalized).cos())
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
    fn bytes_input_is_decoded_and_resampled_to_pcm() {
        let wav = Bytes::from(fixture_bytes("tone-44k-stereo.wav"));

        let prepared =
            prepare_short_audio_bytes_with_limits(wav, "audio/wav", ShortAudioLimits::unbounded())
                .unwrap();

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
    fn windowed_sinc_resample_changes_sample_count_for_new_rate() {
        let resampled = windowed_sinc_resample(&[0.0, 0.5, 1.0, 0.5], 8_000, 16_000);
        assert!(resampled.len() > 4);
    }

    #[test]
    fn windowed_sinc_resample_spreads_impulse_across_kernel() {
        let mut samples = vec![0.0; 64];
        samples[20] = 1.0;

        let resampled = windowed_sinc_resample(&samples, 48_000, 16_000);
        let non_zero_samples = resampled
            .iter()
            .filter(|sample| sample.abs() > 0.0001)
            .count();

        assert!(non_zero_samples > 1);
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
