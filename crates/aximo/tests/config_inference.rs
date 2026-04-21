use aximo::config::Settings;

#[test]
fn default_settings_include_local_model_engines() {
    let settings = Settings::default();

    assert_eq!(settings.inference.models_dir, "/var/lib/aximo/models");
    assert_eq!(settings.inference.default_offline_engine, "parakeet");
    assert_eq!(settings.inference.default_realtime_engine, "parakeet");
    assert!(settings.inference.engines.contains_key("parakeet"));
    assert!(settings.inference.engines.contains_key("gigaam"));
}
