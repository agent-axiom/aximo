use std::{collections::BTreeMap, env, fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Settings {
    pub server: ServerSettings,
    pub limits: LimitSettings,
    pub inference: InferenceSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerSettings::default(),
            limits: LimitSettings::default(),
            inference: InferenceSettings::default(),
        }
    }
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
pub struct LimitSettings {
    pub max_short_audio_requests: usize,
    pub max_realtime_sessions: usize,
}

impl Default for LimitSettings {
    fn default() -> Self {
        Self {
            max_short_audio_requests: 8,
            max_realtime_sessions: 24,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
