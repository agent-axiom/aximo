use aximo_core::EngineCapabilities;
use axum::{extract::State, Json};
use serde::Serialize;

use crate::app::AppState;

#[derive(Debug, Serialize)]
pub struct CapabilitiesResponse {
    pub offline: EngineRoleCapabilities,
    pub realtime: EngineRoleCapabilities,
}

#[derive(Debug, Serialize)]
pub struct EngineRoleCapabilities {
    pub configured_engine: String,
    pub model: EngineCapabilities,
    /// `native_streaming` when the backend itself supports incremental decode;
    /// otherwise `bounded_buffered_offline` for the current rolling-window path.
    pub mode: String,
}

pub async fn capabilities(State(state): State<AppState>) -> Json<CapabilitiesResponse> {
    let offline_capabilities = state.offline_engine.capabilities();
    let realtime_capabilities = state.realtime_engine.capabilities();

    Json(CapabilitiesResponse {
        offline: EngineRoleCapabilities {
            configured_engine: state.offline_engine_name,
            model: offline_capabilities,
            mode: "offline".to_string(),
        },
        realtime: EngineRoleCapabilities {
            configured_engine: state.realtime_engine_name,
            mode: realtime_mode(&realtime_capabilities).to_string(),
            model: realtime_capabilities,
        },
    })
}

fn realtime_mode(capabilities: &EngineCapabilities) -> &'static str {
    if capabilities.supports_native_streaming {
        "native_streaming"
    } else {
        "bounded_buffered_offline"
    }
}
