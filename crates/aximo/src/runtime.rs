use std::sync::Arc;

use aximo_inference::{
    engine::{InferenceError, SpeechEngine},
    runtime::{EngineKind, EngineSpec, RuntimeEngineFactory},
};
use thiserror::Error;

use crate::config::Settings;

#[derive(Debug, Error)]
pub enum RuntimeConfigError {
    #[error("engine {0} is not configured")]
    MissingEngine(String),
    #[error(transparent)]
    Inference(#[from] InferenceError),
}

pub fn resolve_engine_spec(
    settings: &Settings,
    engine_name: &str,
) -> Result<EngineSpec, RuntimeConfigError> {
    let configured = settings
        .inference
        .engines
        .get(engine_name)
        .ok_or_else(|| RuntimeConfigError::MissingEngine(engine_name.to_string()))?;
    let kind: EngineKind = configured.kind.parse()?;

    Ok(EngineSpec {
        kind,
        model_path: std::path::Path::new(&settings.inference.models_dir).join(&configured.path),
    })
}

pub fn load_engine(
    settings: &Settings,
    engine_name: &str,
) -> anyhow::Result<Arc<dyn SpeechEngine>> {
    let spec = resolve_engine_spec(settings, engine_name)?;

    RuntimeEngineFactory
        .build(&spec)
        .map_err(anyhow::Error::new)
}

pub async fn run_service() -> anyhow::Result<()> {
    let settings = Settings::load()?;
    let offline_engine = load_engine(&settings, &settings.inference.default_offline_engine)?;
    let realtime_engine = load_engine(&settings, &settings.inference.default_realtime_engine)?;
    let bind_address = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    let app = crate::app::build_app(settings, offline_engine, realtime_engine);

    axum::serve(listener, app).await?;
    Ok(())
}
