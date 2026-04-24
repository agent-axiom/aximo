//! Audio decode and normalization helpers for Aximo short-audio ingest.

mod decode;
mod normalize;

pub use decode::{decode_container, decode_container_with_sample_limit, AudioError, DecodedAudio};
pub use normalize::{
    prepare_short_audio, prepare_short_audio_with_limits, PreparedAudio, ShortAudioLimits,
};
