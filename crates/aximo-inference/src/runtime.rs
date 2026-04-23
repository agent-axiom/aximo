use std::{
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Instant,
};

use aximo_core::{ShortAudioRequest, ShortAudioResult};
use hound::{SampleFormat, WavSpec, WavWriter};
use tempfile::NamedTempFile;
use transcribe_rs::{
    onnx::{gigaam::GigaAMModel, parakeet::ParakeetModel, Quantization},
    SpeechModel as TranscribeSpeechModel, TranscribeOptions,
};

use crate::engine::{InferenceError, SpeechEngine};

// Current transcribe-rs ONNX integrations used here return plain transcript text
// to this adapter path. They do not expose per-segment timestamps or detected
// language through the current model adapter interface, so this runtime keeps
// `segments` empty and `detected_language` as `None` while still reporting
// measured input duration and processing time.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Parakeet,
    Gigaam,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parakeet => "parakeet",
            Self::Gigaam => "gigaam",
        }
    }
}

impl FromStr for EngineKind {
    type Err = InferenceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "parakeet" => Ok(Self::Parakeet),
            "gigaam" => Ok(Self::Gigaam),
            other => Err(InferenceError::UnsupportedEngine(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSpec {
    pub kind: EngineKind,
    pub model_path: PathBuf,
}

#[derive(Default)]
pub struct RuntimeEngineFactory;

impl RuntimeEngineFactory {
    pub fn build(&self, spec: &EngineSpec) -> Result<Arc<dyn SpeechEngine>, InferenceError> {
        if !spec.model_path.exists() {
            return Err(InferenceError::Unavailable(format!(
                "model path {} does not exist",
                spec.model_path.display()
            )));
        }

        let model: Box<dyn TranscribeSpeechModel + Send> = match spec.kind {
            EngineKind::Parakeet => Box::new(
                ParakeetModel::load(&spec.model_path, &Quantization::Int8)
                    .map_err(|error| InferenceError::Runtime(error.to_string()))?,
            ),
            EngineKind::Gigaam => Box::new(
                GigaAMModel::load(&spec.model_path, &Quantization::default())
                    .map_err(|error| InferenceError::Runtime(error.to_string()))?,
            ),
        };

        Ok(Arc::new(TranscribeRsEngine {
            engine_name: spec.kind.as_str().to_string(),
            model: Mutex::new(TranscribeRsModelRunner { model }),
        }))
    }
}

trait ModelRunner: Send {
    fn transcribe_file(
        &mut self,
        path: &Path,
        language: Option<String>,
    ) -> Result<String, InferenceError>;
}

struct TranscribeRsModelRunner {
    model: Box<dyn TranscribeSpeechModel + Send>,
}

impl ModelRunner for TranscribeRsModelRunner {
    fn transcribe_file(
        &mut self,
        path: &Path,
        language: Option<String>,
    ) -> Result<String, InferenceError> {
        let options = TranscribeOptions {
            language,
            ..Default::default()
        };
        let path = path.to_path_buf();
        let result = self
            .model
            .transcribe_file(&path, &options)
            .map_err(|error| InferenceError::Runtime(error.to_string()))?;

        Ok(result.text)
    }
}

struct TranscribeRsEngine<R = TranscribeRsModelRunner> {
    engine_name: String,
    model: Mutex<R>,
}

impl<R: ModelRunner> SpeechEngine for TranscribeRsEngine<R> {
    fn transcribe_short(
        &self,
        request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError> {
        let started_at = Instant::now();
        let wav_file = materialize_as_wav(&request)?;
        let wav_path = wav_file.path();
        let duration_ms = wav_duration_ms(wav_path)?;

        let mut model = self.model.lock().expect("transcribe-rs model lock");
        let text = model.transcribe_file(wav_path, request.language_hint.clone())?;

        Ok(ShortAudioResult {
            text,
            segments: Vec::new(),
            detected_language: None,
            engine: self.engine_name.clone(),
            duration_ms,
            processing_ms: started_at.elapsed().as_millis() as u64,
        })
    }
}

fn wav_duration_ms(path: &Path) -> Result<u64, InferenceError> {
    let reader = hound::WavReader::open(path).map_err(io_error)?;
    let spec = reader.spec();
    let sample_rate = u64::from(spec.sample_rate);
    let channels = u64::from(spec.channels);

    if sample_rate == 0 || channels == 0 {
        return Err(InferenceError::InvalidAudio(
            "wav metadata must declare non-zero sample rate and channels".to_string(),
        ));
    }

    let frames = u64::from(reader.duration()) / channels;
    Ok(frames.saturating_mul(1000) / sample_rate)
}

fn materialize_as_wav(request: &ShortAudioRequest) -> Result<NamedTempFile, InferenceError> {
    let mut file = NamedTempFile::new().map_err(io_error)?;

    if request.content_type.contains("wav") {
        file.write_all(&request.audio_bytes).map_err(io_error)?;
        file.flush().map_err(io_error)?;
        return Ok(file);
    }

    if request.content_type.contains("pcm") || request.content_type.contains("octet-stream") {
        write_pcm_as_wav(file.path(), &request.audio_bytes)?;
        return Ok(file);
    }

    Err(InferenceError::InvalidAudio(format!(
        "unsupported content type {}",
        request.content_type
    )))
}

fn write_pcm_as_wav(path: &Path, bytes: &[u8]) -> Result<(), InferenceError> {
    if bytes.len() % 2 != 0 {
        return Err(InferenceError::InvalidAudio(
            "pcm payload must be aligned to 16-bit samples".to_string(),
        ));
    }

    let spec = WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).map_err(io_error)?;

    for chunk in bytes.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        writer.write_sample(sample).map_err(io_error)?;
    }

    writer.finalize().map_err(io_error)?;
    Ok(())
}

fn io_error(error: impl ToString) -> InferenceError {
    InferenceError::Runtime(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    struct FakeRunner {
        text: String,
        seen_header: Option<[u8; 4]>,
        sleep_for: Option<Duration>,
    }

    impl FakeRunner {
        fn new(text: &str) -> Self {
            Self {
                text: text.to_string(),
                seen_header: None,
                sleep_for: None,
            }
        }

        fn with_delay(text: &str, sleep_for: Duration) -> Self {
            Self {
                text: text.to_string(),
                seen_header: None,
                sleep_for: Some(sleep_for),
            }
        }
    }

    impl ModelRunner for FakeRunner {
        fn transcribe_file(
            &mut self,
            path: &Path,
            _language: Option<String>,
        ) -> Result<String, InferenceError> {
            let bytes = std::fs::read(path).map_err(io_error)?;
            self.seen_header = Some(bytes[0..4].try_into().expect("wav header"));
            if let Some(delay) = self.sleep_for {
                std::thread::sleep(delay);
            }
            Ok(self.text.clone())
        }
    }

    #[test]
    fn engine_kind_parses_and_formats_known_values() {
        assert_eq!(
            EngineKind::from_str("parakeet").unwrap(),
            EngineKind::Parakeet
        );
        assert_eq!(EngineKind::from_str("gigaam").unwrap(), EngineKind::Gigaam);
        assert_eq!(EngineKind::Parakeet.as_str(), "parakeet");
        assert!(EngineKind::from_str("unknown").is_err());
    }

    #[test]
    fn materialize_as_wav_rejects_unsupported_content_type() {
        let request = ShortAudioRequest {
            audio_bytes: vec![1, 2, 3, 4],
            content_type: "audio/mp3".to_string(),
            engine: None,
            language_hint: None,
            timestamps: false,
        };

        let error = materialize_as_wav(&request).unwrap_err();
        assert!(error.to_string().contains("unsupported content type"));
    }

    #[test]
    fn materialize_as_wav_rejects_odd_length_pcm() {
        let request = ShortAudioRequest {
            audio_bytes: vec![1, 2, 3],
            content_type: "audio/pcm".to_string(),
            engine: None,
            language_hint: None,
            timestamps: false,
        };

        let error = materialize_as_wav(&request).unwrap_err();
        assert!(error.to_string().contains("aligned to 16-bit"));
    }

    #[test]
    fn materialize_as_wav_passes_through_existing_wav_payload() {
        let request = ShortAudioRequest {
            audio_bytes: b"RIFFdemo".to_vec(),
            content_type: "audio/wav".to_string(),
            engine: None,
            language_hint: None,
            timestamps: false,
        };

        let file = materialize_as_wav(&request).unwrap();
        let bytes = std::fs::read(file.path()).unwrap();

        assert_eq!(bytes, b"RIFFdemo");
    }

    #[test]
    fn transcribe_short_converts_pcm_payload_to_wav_before_running_model() {
        let engine = TranscribeRsEngine {
            engine_name: "fake".to_string(),
            model: Mutex::new(FakeRunner::new("decoded text")),
        };
        let request = ShortAudioRequest {
            audio_bytes: vec![0, 0, 1, 0, 2, 0, 3, 0],
            content_type: "audio/pcm".to_string(),
            engine: None,
            language_hint: Some("ru".to_string()),
            timestamps: false,
        };

        let result = engine.transcribe_short(request).unwrap();

        assert_eq!(result.text, "decoded text");
        assert!(result.segments.is_empty());
        assert_eq!(result.detected_language, None);
        assert_eq!(result.engine, "fake");
        assert_eq!(engine.model.lock().unwrap().seen_header, Some(*b"RIFF"));
    }

    #[test]
    fn transcribe_short_reports_measured_duration_and_processing_time() {
        let engine = TranscribeRsEngine {
            engine_name: "fake".to_string(),
            model: Mutex::new(FakeRunner::with_delay(
                "decoded text",
                Duration::from_millis(5),
            )),
        };
        let request = ShortAudioRequest {
            audio_bytes: vec![0_u8; 32_000],
            content_type: "audio/pcm".to_string(),
            engine: None,
            language_hint: None,
            timestamps: false,
        };

        let result = engine.transcribe_short(request).unwrap();

        assert_eq!(result.duration_ms, 1_000);
        assert!(result.processing_ms >= 5);
    }
}
