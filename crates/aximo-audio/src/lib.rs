//! Audio decode and normalization helpers for Aximo short-audio ingest.

mod decode;
mod normalize;

pub use decode::{decode_container, AudioError, DecodedAudio};
pub use normalize::{prepare_short_audio, PreparedAudio};
