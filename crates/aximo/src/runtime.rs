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

pub fn load_default_engines(
    settings: &Settings,
) -> anyhow::Result<(Arc<dyn SpeechEngine>, Arc<dyn SpeechEngine>)> {
    load_default_engines_with_factory(settings, |spec| {
        RuntimeEngineFactory.build(spec).map_err(anyhow::Error::new)
    })
}

fn load_default_engines_with_factory(
    settings: &Settings,
    mut build: impl FnMut(&EngineSpec) -> anyhow::Result<Arc<dyn SpeechEngine>>,
) -> anyhow::Result<(Arc<dyn SpeechEngine>, Arc<dyn SpeechEngine>)> {
    let offline_spec = resolve_engine_spec(settings, &settings.inference.default_offline_engine)?;
    let realtime_spec = resolve_engine_spec(settings, &settings.inference.default_realtime_engine)?;

    let offline_engine = build(&offline_spec)?;
    if offline_spec == realtime_spec {
        return Ok((Arc::clone(&offline_engine), offline_engine));
    }

    let realtime_engine = build(&realtime_spec)?;
    Ok((offline_engine, realtime_engine))
}

pub async fn run_service() -> anyhow::Result<()> {
    let settings = Settings::load()?;
    let (offline_engine, realtime_engine) = load_default_engines(&settings)?;
    let bind_address = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    let app = crate::app::build_app(settings, offline_engine, realtime_engine);

    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use aximo_inference::engine::{FakeEngine, SpeechEngine};

    use super::*;

    #[test]
    fn default_engine_loader_reuses_matching_engine_specs() {
        let settings = Settings::default();
        let build_count = AtomicUsize::new(0);

        let (offline, realtime) = load_default_engines_with_factory(&settings, |_spec| {
            build_count.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(FakeEngine) as Arc<dyn SpeechEngine>)
        })
        .unwrap();

        assert!(Arc::ptr_eq(&offline, &realtime));
        assert_eq!(build_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn default_engine_loader_loads_distinct_engine_specs_separately() {
        let mut settings = Settings::default();
        settings.inference.default_realtime_engine = "gigaam".to_string();
        let build_count = AtomicUsize::new(0);

        let (offline, realtime) = load_default_engines_with_factory(&settings, |_spec| {
            build_count.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(FakeEngine) as Arc<dyn SpeechEngine>)
        })
        .unwrap();

        assert!(!Arc::ptr_eq(&offline, &realtime));
        assert_eq!(build_count.load(Ordering::SeqCst), 2);
    }
}
