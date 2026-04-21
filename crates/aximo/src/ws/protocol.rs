use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ClientEvent {
    pub event: String,
}

#[derive(Debug, Serialize)]
pub struct ServerEvent {
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ServerEvent {
    pub fn session_started(session_id: String) -> Self {
        Self {
            event: "session_started".to_string(),
            session_id: Some(session_id),
            text: None,
        }
    }

    pub fn final_text(text: impl Into<String>) -> Self {
        Self {
            event: "final".to_string(),
            session_id: None,
            text: Some(text.into()),
        }
    }

    pub fn partial_text(text: impl Into<String>) -> Self {
        Self {
            event: "partial".to_string(),
            session_id: None,
            text: Some(text.into()),
        }
    }

    pub fn error() -> Self {
        Self {
            event: "error".to_string(),
            session_id: None,
            text: None,
        }
    }
}
