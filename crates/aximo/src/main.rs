use aximo::{app::build_app, config::Settings};
use aximo_inference::runtime::{EngineKind, EngineSpec, RuntimeEngineFactory};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let settings = Settings::load()?;
    let offline_engine = load_engine(&settings, &settings.inference.default_offline_engine)?;
    let realtime_engine = load_engine(&settings, &settings.inference.default_realtime_engine)?;
    let bind_address = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    let app = build_app(settings, offline_engine, realtime_engine);

    axum::serve(listener, app).await?;
    Ok(())
}

fn load_engine(
    settings: &Settings,
    engine_name: &str,
) -> anyhow::Result<std::sync::Arc<dyn aximo_inference::engine::SpeechEngine>> {
    let configured = settings
        .inference
        .engines
        .get(engine_name)
        .ok_or_else(|| anyhow::anyhow!("engine {engine_name} is not configured"))?;
    let kind: EngineKind = configured.kind.parse()?;
    let spec = EngineSpec {
        kind,
        model_path: std::path::Path::new(&settings.inference.models_dir).join(&configured.path),
    };

    RuntimeEngineFactory::default()
        .build(&spec)
        .map_err(|error| anyhow::anyhow!(error))
}
