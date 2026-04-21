use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Settings {
    pub server: ServerSettings,
    pub limits: LimitSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerSettings::default(),
            limits: LimitSettings::default(),
        }
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
