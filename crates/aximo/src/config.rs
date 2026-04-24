use std::{collections::BTreeMap, env, fs, path::Path, str::FromStr};

use anyhow::Context;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub server: ServerSettings,
    pub limits: LimitSettings,
    pub inference: InferenceSettings,
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        let mut settings = match env::var("AXIMO_CONFIG") {
            Ok(path) => Self::from_path(path),
            Err(_) => Ok(Self::default()),
        }?;
        settings.apply_env_overrides()?;
        Ok(settings)
    }

    pub fn from_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        let settings = toml::from_str::<Self>(&contents)?;
        Ok(settings)
    }

    fn apply_env_overrides(&mut self) -> anyhow::Result<()> {
        if let Some(value) = env_string("AXIMO_SERVER_HOST")? {
            self.server.host = value;
        }
        if let Some(value) = env_parse("AXIMO_SERVER_PORT")? {
            self.server.port = value;
        }
        if let Some(value) = env_string("AXIMO_MODELS_DIR")? {
            self.inference.models_dir = value;
        }
        if let Some(value) = env_string("AXIMO_DEFAULT_OFFLINE_ENGINE")? {
            self.inference.default_offline_engine = value;
        }
        if let Some(value) = env_string("AXIMO_DEFAULT_REALTIME_ENGINE")? {
            self.inference.default_realtime_engine = value;
        }
        if let Some(value) = env_parse("AXIMO_MAX_SHORT_AUDIO_REQUESTS")? {
            self.limits.max_short_audio_requests = value;
        }
        if let Some(value) = env_parse("AXIMO_MAX_REALTIME_SESSIONS")? {
            self.limits.max_realtime_sessions = value;
        }
        if let Some(value) = env_parse("AXIMO_MAX_SHORT_INFERENCES")? {
            self.limits.max_short_inferences = value;
        }
        if let Some(value) = env_parse("AXIMO_MAX_REALTIME_INFERENCES")? {
            self.limits.max_realtime_inferences = value;
        }
        if let Some(value) = env_parse("AXIMO_MAX_REALTIME_SESSION_BYTES")? {
            self.limits.max_realtime_session_bytes = value;
        }
        if let Some(value) = env_parse("AXIMO_MAX_REALTIME_SESSION_DURATION_MS")? {
            self.limits.max_realtime_session_duration_ms = value;
        }
        if let Some(value) = env_parse("AXIMO_REALTIME_PARTIAL_MIN_INTERVAL_MS")? {
            self.limits.realtime_partial_min_interval_ms = value;
        }
        if let Some(value) = env_parse("AXIMO_REALTIME_PARTIAL_MIN_CHUNK_BYTES")? {
            self.limits.realtime_partial_min_chunk_bytes = value;
        }

        Ok(())
    }
}

fn env_string(name: &str) -> anyhow::Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("invalid {name}")),
    }
}

fn env_parse<T>(name: &str) -> anyhow::Result<Option<T>>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let Some(value) = env_string(name)? else {
        return Ok(None);
    };

    value
        .parse::<T>()
        .map(Some)
        .map_err(|error| anyhow::anyhow!("invalid {name} value {value:?}: {error}"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LimitSettings {
    pub max_short_audio_requests: usize,
    pub max_realtime_sessions: usize,
    pub max_short_inferences: usize,
    pub max_realtime_inferences: usize,
    pub max_realtime_session_bytes: usize,
    pub max_realtime_session_duration_ms: u64,
    pub realtime_partial_min_interval_ms: u64,
    pub realtime_partial_min_chunk_bytes: usize,
}

impl Default for LimitSettings {
    fn default() -> Self {
        Self {
            max_short_audio_requests: 8,
            max_realtime_sessions: 24,
            max_short_inferences: 1,
            max_realtime_inferences: 1,
            max_realtime_session_bytes: 1_920_000,
            max_realtime_session_duration_ms: 60_000,
            realtime_partial_min_interval_ms: 300,
            realtime_partial_min_chunk_bytes: 9_600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct InferenceSettings {
    pub models_dir: String,
    pub default_offline_engine: String,
    pub default_realtime_engine: String,
    pub engines: BTreeMap<String, ConfiguredEngine>,
}

impl Default for InferenceSettings {
    fn default() -> Self {
        let mut engines = BTreeMap::new();
        engines.insert(
            "parakeet".to_string(),
            ConfiguredEngine {
                kind: "parakeet".to_string(),
                path: "parakeet-tdt-0.6b-v3-int8".to_string(),
            },
        );
        engines.insert(
            "gigaam".to_string(),
            ConfiguredEngine {
                kind: "gigaam".to_string(),
                path: "giga-am-v3".to_string(),
            },
        );

        Self {
            models_dir: "/var/lib/aximo/models".to_string(),
            default_offline_engine: "parakeet".to_string(),
            default_realtime_engine: "parakeet".to_string(),
            engines,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfiguredEngine {
    pub kind: String,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::*;

    const OVERLAY_ENV_VARS: &[&str] = &[
        "AXIMO_CONFIG",
        "AXIMO_SERVER_HOST",
        "AXIMO_SERVER_PORT",
        "AXIMO_MODELS_DIR",
        "AXIMO_DEFAULT_OFFLINE_ENGINE",
        "AXIMO_DEFAULT_REALTIME_ENGINE",
        "AXIMO_MAX_SHORT_AUDIO_REQUESTS",
        "AXIMO_MAX_REALTIME_SESSIONS",
        "AXIMO_MAX_SHORT_INFERENCES",
        "AXIMO_MAX_REALTIME_INFERENCES",
        "AXIMO_MAX_REALTIME_SESSION_BYTES",
        "AXIMO_MAX_REALTIME_SESSION_DURATION_MS",
        "AXIMO_REALTIME_PARTIAL_MIN_INTERVAL_MS",
        "AXIMO_REALTIME_PARTIAL_MIN_CHUNK_BYTES",
    ];

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_overlay_env() {
        for key in OVERLAY_ENV_VARS {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn load_returns_defaults_when_env_is_missing() {
        let _guard = env_lock().lock().unwrap();
        clear_overlay_env();

        let settings = Settings::load().unwrap();

        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn load_reads_config_path_from_env() {
        let _guard = env_lock().lock().unwrap();
        clear_overlay_env();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("aximo.toml");
        std::fs::write(
            &path,
            r#"
[server]
host = "127.0.0.1"
port = 8081

[inference]
models_dir = "/tmp/models"
default_offline_engine = "parakeet"
default_realtime_engine = "gigaam"

[inference.engines.parakeet]
kind = "parakeet"
path = "parakeet-tdt-0.6b-v3-int8"

[inference.engines.gigaam]
kind = "gigaam"
path = "giga-am-v3"

[limits]
max_short_audio_requests = 2
max_realtime_sessions = 1
"#,
        )
        .unwrap();
        std::env::set_var("AXIMO_CONFIG", &path);

        let settings = Settings::load().unwrap();

        assert_eq!(settings.server.port, 8081);
        assert_eq!(settings.inference.models_dir, "/tmp/models");

        clear_overlay_env();
    }

    #[test]
    fn load_applies_per_field_env_overlay_after_toml() {
        let _guard = env_lock().lock().unwrap();
        clear_overlay_env();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("aximo.toml");
        std::fs::write(
            &path,
            r#"
[server]
host = "127.0.0.1"
port = 8081

[inference]
models_dir = "/tmp/models"
default_offline_engine = "parakeet"
default_realtime_engine = "parakeet"

[inference.engines.parakeet]
kind = "parakeet"
path = "parakeet-tdt-0.6b-v3-int8"

[inference.engines.gigaam]
kind = "gigaam"
path = "giga-am-v3"

[limits]
max_short_audio_requests = 2
max_realtime_sessions = 3
max_short_inferences = 1
max_realtime_inferences = 1
max_realtime_session_bytes = 1000
max_realtime_session_duration_ms = 2000
realtime_partial_min_interval_ms = 300
realtime_partial_min_chunk_bytes = 400
"#,
        )
        .unwrap();

        std::env::set_var("AXIMO_CONFIG", &path);
        std::env::set_var("AXIMO_SERVER_HOST", "0.0.0.0");
        std::env::set_var("AXIMO_SERVER_PORT", "9090");
        std::env::set_var("AXIMO_MODELS_DIR", "/mnt/models");
        std::env::set_var("AXIMO_DEFAULT_OFFLINE_ENGINE", "gigaam");
        std::env::set_var("AXIMO_DEFAULT_REALTIME_ENGINE", "parakeet");
        std::env::set_var("AXIMO_MAX_SHORT_AUDIO_REQUESTS", "9");
        std::env::set_var("AXIMO_MAX_REALTIME_SESSIONS", "10");
        std::env::set_var("AXIMO_MAX_SHORT_INFERENCES", "2");
        std::env::set_var("AXIMO_MAX_REALTIME_INFERENCES", "3");
        std::env::set_var("AXIMO_MAX_REALTIME_SESSION_BYTES", "123456");
        std::env::set_var("AXIMO_MAX_REALTIME_SESSION_DURATION_MS", "654321");
        std::env::set_var("AXIMO_REALTIME_PARTIAL_MIN_INTERVAL_MS", "150");
        std::env::set_var("AXIMO_REALTIME_PARTIAL_MIN_CHUNK_BYTES", "8192");

        let settings = Settings::load().unwrap();

        assert_eq!(settings.server.host, "0.0.0.0");
        assert_eq!(settings.server.port, 9090);
        assert_eq!(settings.inference.models_dir, "/mnt/models");
        assert_eq!(settings.inference.default_offline_engine, "gigaam");
        assert_eq!(settings.inference.default_realtime_engine, "parakeet");
        assert_eq!(settings.limits.max_short_audio_requests, 9);
        assert_eq!(settings.limits.max_realtime_sessions, 10);
        assert_eq!(settings.limits.max_short_inferences, 2);
        assert_eq!(settings.limits.max_realtime_inferences, 3);
        assert_eq!(settings.limits.max_realtime_session_bytes, 123456);
        assert_eq!(settings.limits.max_realtime_session_duration_ms, 654321);
        assert_eq!(settings.limits.realtime_partial_min_interval_ms, 150);
        assert_eq!(settings.limits.realtime_partial_min_chunk_bytes, 8192);

        clear_overlay_env();
    }

    #[test]
    fn load_returns_contextual_error_for_invalid_numeric_env_overlay() {
        let _guard = env_lock().lock().unwrap();
        clear_overlay_env();
        std::env::set_var("AXIMO_SERVER_PORT", "not-a-port");

        let error = Settings::load().unwrap_err();

        assert!(error.to_string().contains("AXIMO_SERVER_PORT"));

        clear_overlay_env();
    }
}
