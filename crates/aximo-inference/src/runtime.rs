use std::{
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
};

use aximo_core::{ShortAudioRequest, ShortAudioResult};
use hound::{SampleFormat, WavSpec, WavWriter};
use tempfile::NamedTempFile;
use transcribe_rs::{
    onnx::{gigaam::GigaAMModel, parakeet::ParakeetModel, Quantization},
    SpeechModel as TranscribeSpeechModel, TranscribeOptions,
};

use crate::engine::{InferenceError, SpeechEngine};

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
            model: Mutex::new(model),
        }))
    }
}

struct TranscribeRsEngine {
    engine_name: String,
    model: Mutex<Box<dyn TranscribeSpeechModel + Send>>,
}

impl SpeechEngine for TranscribeRsEngine {
    fn transcribe_short(
        &self,
        request: ShortAudioRequest,
    ) -> Result<ShortAudioResult, InferenceError> {
        let wav_file = materialize_as_wav(&request)?;
        let wav_path = wav_file.path().to_path_buf();
        let options = TranscribeOptions {
            language: request.language_hint.clone(),
            ..Default::default()
        };

        let mut model = self.model.lock().expect("transcribe-rs model lock");
        let result = model
            .transcribe_file(&wav_path, &options)
            .map_err(|error| InferenceError::Runtime(error.to_string()))?;

        Ok(ShortAudioResult {
            text: result.text,
            segments: Vec::new(),
            detected_language: request.language_hint.unwrap_or_else(|| "auto".to_string()),
            engine: self.engine_name.clone(),
            duration_ms: 0,
            processing_ms: 0,
        })
    }
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
