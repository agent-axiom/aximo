use std::path::PathBuf;

use aximo_audio::{prepare_short_audio, AudioError};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(name)).unwrap()
}

fn assert_pcm_output(bytes: &[u8]) {
    assert!(!bytes.is_empty());
    assert_eq!(bytes.len() % 2, 0);
    assert!((4_000..=20_000).contains(&bytes.len()));
}

#[test]
fn wav_fixture_is_passed_through_when_already_model_ready() {
    let wav = fixture_bytes("tone-16k-mono.wav");

    let prepared = prepare_short_audio(&wav, "audio/wav").unwrap();

    assert_eq!(prepared.content_type, "audio/wav");
    assert_eq!(prepared.audio_bytes, wav);
}

#[test]
fn wav_fixture_is_normalized_when_it_is_not_model_ready() {
    let wav = fixture_bytes("tone-44k-stereo.wav");

    let prepared = prepare_short_audio(&wav, "audio/wav").unwrap();

    assert_eq!(prepared.content_type, "audio/pcm");
    assert_pcm_output(&prepared.audio_bytes);
}

#[test]
fn mp3_fixture_decodes_to_normalized_pcm() {
    let mp3 = fixture_bytes("tone-16k-mono.mp3");

    let prepared = prepare_short_audio(&mp3, "audio/mpeg").unwrap();

    assert_eq!(prepared.content_type, "audio/pcm");
    assert_pcm_output(&prepared.audio_bytes);
}

#[test]
fn flac_fixture_decodes_to_normalized_pcm() {
    let flac = fixture_bytes("tone-16k-mono.flac");

    let prepared = prepare_short_audio(&flac, "audio/flac").unwrap();

    assert_eq!(prepared.content_type, "audio/pcm");
    assert_pcm_output(&prepared.audio_bytes);
}

#[test]
fn m4a_fixture_decodes_to_normalized_pcm() {
    let m4a = fixture_bytes("tone-16k-mono.m4a");

    let prepared = prepare_short_audio(&m4a, "audio/mp4").unwrap();

    assert_eq!(prepared.content_type, "audio/pcm");
    assert_pcm_output(&prepared.audio_bytes);
}

#[test]
fn corrupt_fixture_is_rejected() {
    let corrupt = fixture_bytes("corrupt.mp3");

    let error = prepare_short_audio(&corrupt, "audio/mpeg").unwrap_err();
    assert!(matches!(error, AudioError::Decode(_)));
}

#[test]
fn odd_length_raw_pcm_is_rejected() {
    let error = prepare_short_audio(&[1_u8, 2, 3], "audio/pcm").unwrap_err();
    assert!(matches!(error, AudioError::InvalidPcm(_)));
}

#[test]
fn unsupported_content_type_is_rejected() {
    let error = prepare_short_audio(b"{}", "application/json").unwrap_err();
    assert!(matches!(error, AudioError::UnsupportedContentType(_)));
}
