use std::{fs, path::PathBuf};

use aximo::{
    config::Settings,
    runtime::{resolve_engine_spec, RuntimeConfigError},
};
use aximo_inference::runtime::EngineKind;

#[test]
fn settings_can_be_loaded_from_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aximo.toml");
    fs::write(
        &path,
        r#"
[server]
host = "127.0.0.1"
port = 9090

[inference]
models_dir = "/srv/models"
default_offline_engine = "gigaam"
default_realtime_engine = "parakeet"

[inference.engines.parakeet]
kind = "parakeet"
path = "parakeet-tdt-0.6b-v3-int8"

[inference.engines.gigaam]
kind = "gigaam"
path = "giga-am-v3"

[limits]
max_short_audio_requests = 4
max_short_audio_bytes = 12000000
max_short_raw_pcm_bytes = 960000
max_short_audio_duration_ms = 30000
max_short_decoded_samples = 2880000
max_realtime_sessions = 2
max_short_inferences = 1
max_realtime_inferences = 1
max_realtime_session_bytes = 960000
max_realtime_session_duration_ms = 30000
realtime_partial_min_interval_ms = 450
realtime_partial_min_chunk_bytes = 12000
"#,
    )
    .unwrap();

    let settings = Settings::from_path(&path).unwrap();

    assert_eq!(settings.server.host, "127.0.0.1");
    assert_eq!(settings.server.port, 9090);
    assert_eq!(settings.inference.models_dir, "/srv/models");
    assert_eq!(settings.limits.max_short_audio_bytes, 12000000);
    assert_eq!(settings.limits.max_short_raw_pcm_bytes, 960000);
    assert_eq!(settings.limits.max_short_audio_duration_ms, 30000);
    assert_eq!(settings.limits.max_short_decoded_samples, 2880000);
    assert_eq!(settings.limits.max_realtime_sessions, 2);
    assert_eq!(settings.limits.max_short_inferences, 1);
    assert_eq!(settings.limits.max_short_audio_bytes, 25_000_000);
    assert_eq!(settings.limits.max_short_raw_pcm_bytes, 1_920_000);
    assert_eq!(settings.limits.max_short_audio_duration_ms, 60_000);
    assert_eq!(settings.limits.max_short_decoded_samples, 5_760_000);
    assert_eq!(settings.limits.max_realtime_inferences, 1);
    assert_eq!(settings.limits.max_realtime_session_bytes, 960000);
    assert_eq!(settings.limits.max_realtime_session_duration_ms, 30000);
    assert_eq!(settings.limits.realtime_partial_min_interval_ms, 450);
    assert_eq!(settings.limits.realtime_partial_min_chunk_bytes, 12000);
}

#[test]
fn runtime_resolution_builds_engine_spec_from_settings() {
    let settings = Settings::default();

    let spec = resolve_engine_spec(&settings, "gigaam").unwrap();

    assert_eq!(spec.kind, EngineKind::Gigaam);
    assert_eq!(
        spec.model_path,
        PathBuf::from("/var/lib/aximo/models").join("giga-am-v3")
    );
}

#[test]
fn runtime_resolution_rejects_unknown_engine_name() {
    let settings = Settings::default();

    let error = resolve_engine_spec(&settings, "missing").unwrap_err();

    assert!(matches!(error, RuntimeConfigError::MissingEngine(_)));
}

#[test]
fn runtime_loader_returns_error_for_missing_model_path() {
    let settings = Settings::default();

    let error = match aximo::runtime::load_engine(&settings, "parakeet") {
        Ok(_) => panic!("expected load_engine to fail without a local model directory"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("does not exist"));
}

#[test]
fn settings_fill_in_default_inference_limits_when_config_omits_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aximo.toml");
    fs::write(
        &path,
        r#"
[server]
host = "127.0.0.1"
port = 9090

[inference]
models_dir = "/srv/models"
default_offline_engine = "gigaam"
default_realtime_engine = "parakeet"

[inference.engines.parakeet]
kind = "parakeet"
path = "parakeet-tdt-0.6b-v3-int8"

[inference.engines.gigaam]
kind = "gigaam"
path = "giga-am-v3"

[limits]
max_short_audio_requests = 4
max_realtime_sessions = 2
"#,
    )
    .unwrap();

    let settings = Settings::from_path(&path).unwrap();

    assert_eq!(settings.limits.max_short_inferences, 1);
    assert_eq!(settings.limits.max_realtime_inferences, 1);
    assert_eq!(settings.limits.max_realtime_session_bytes, 1_920_000);
    assert_eq!(settings.limits.max_realtime_session_duration_ms, 60_000);
    assert_eq!(settings.limits.realtime_partial_min_interval_ms, 300);
    assert_eq!(settings.limits.realtime_partial_min_chunk_bytes, 9_600);
}
