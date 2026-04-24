//! Audio decode and normalization helpers for Aximo short-audio ingest.

mod decode;
mod media_type;
mod normalize;

pub use decode::{
    decode_container, decode_container_bytes_with_sample_limit, decode_container_with_sample_limit,
    AudioError, DecodedAudio,
};
pub use media_type::{parse_audio_media_type, AudioMediaType};
pub use normalize::{
    prepare_short_audio, prepare_short_audio_bytes_with_limits, prepare_short_audio_with_limits,
    PreparedAudio, ShortAudioLimits,
};
