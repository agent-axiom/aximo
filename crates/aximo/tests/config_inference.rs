use aximo::config::Settings;

#[test]
fn default_settings_include_local_model_engines() {
    let settings = Settings::default();

    assert_eq!(settings.inference.models_dir, "/var/lib/aximo/models");
    assert_eq!(settings.inference.default_offline_engine, "parakeet");
    assert_eq!(settings.inference.default_realtime_engine, "parakeet");
    assert_eq!(settings.limits.max_short_audio_bytes, 25_000_000);
    assert_eq!(settings.limits.max_short_raw_pcm_bytes, 1_920_000);
    assert_eq!(settings.limits.max_short_audio_duration_ms, 60_000);
    assert_eq!(settings.limits.max_short_decoded_samples, 5_760_000);
    assert!(settings.inference.engines.contains_key("parakeet"));
    assert!(settings.inference.engines.contains_key("gigaam"));
}
