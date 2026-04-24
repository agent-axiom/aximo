use aximo_audio::{parse_audio_media_type, AudioError, AudioMediaType};

#[test]
fn parser_accepts_supported_aliases_and_parameters() {
    assert_eq!(
        parse_audio_media_type("audio/wav; codecs=1").unwrap(),
        AudioMediaType::Wav
    );
    assert_eq!(
        parse_audio_media_type(" audio/x-wav ").unwrap(),
        AudioMediaType::Wav
    );
    assert_eq!(
        parse_audio_media_type("audio/mpeg").unwrap(),
        AudioMediaType::Mp3
    );
    assert_eq!(
        parse_audio_media_type("application/octet-stream").unwrap(),
        AudioMediaType::RawPcm
    );
}

#[test]
fn parser_rejects_bogus_and_empty_content_types() {
    assert!(matches!(
        parse_audio_media_type("application/json"),
        Err(AudioError::UnsupportedContentType(_))
    ));
    assert!(matches!(
        parse_audio_media_type(""),
        Err(AudioError::UnsupportedContentType(_))
    ));
}
