use std::{collections::BTreeMap, env, fs, path::Path};

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
        match env::var("AXIMO_CONFIG") {
            Ok(path) => Self::from_path(path),
            Err(_) => Ok(Self::default()),
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        let settings = toml::from_str::<Self>(&contents)?;
        Ok(settings)
    }
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

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn load_returns_defaults_when_env_is_missing() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("AXIMO_CONFIG");

        let settings = Settings::load().unwrap();

        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn load_reads_config_path_from_env() {
        let _guard = env_lock().lock().unwrap();
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

        std::env::remove_var("AXIMO_CONFIG");
    }
}
