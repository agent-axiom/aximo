use std::{
    future::{Future, IntoFuture},
    sync::Arc,
    time::Duration,
};

use aximo_inference::{
    engine::{InferenceError, SpeechEngine},
    runtime::{EngineKind, EngineSpec, RuntimeEngineFactory},
};
use axum::Router;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::config::Settings;

const APP_SHUTDOWN_DRAIN: Duration = Duration::from_millis(25);

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
    let shutdown_grace_period = Duration::from_millis(settings.server.shutdown_grace_period_ms);
    let (app, app_shutdown) =
        crate::app::build_app_with_shutdown(settings, offline_engine, realtime_engine);

    serve_with_shutdown_notifying_app(
        listener,
        app,
        shutdown_signal(),
        shutdown_grace_period,
        app_shutdown,
    )
    .await?;
    Ok(())
}

pub async fn serve_with_shutdown<F>(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: F,
    shutdown_grace_period: Duration,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    serve_with_shutdown_inner(listener, app, shutdown, shutdown_grace_period, None).await
}

pub async fn serve_with_shutdown_notifying_app<F>(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: F,
    shutdown_grace_period: Duration,
    app_shutdown: crate::app::ShutdownHandle,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    serve_with_shutdown_inner(
        listener,
        app,
        shutdown,
        shutdown_grace_period,
        Some(app_shutdown),
    )
    .await
}

async fn serve_with_shutdown_inner<F>(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: F,
    shutdown_grace_period: Duration,
    app_shutdown: Option<crate::app::ShutdownHandle>,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (server_shutdown_tx, server_shutdown_rx) = oneshot::channel::<()>();
    let (grace_started_tx, grace_started_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        shutdown.await;
        if let Some(app_shutdown) = app_shutdown {
            app_shutdown.notify();
            tokio::time::sleep(APP_SHUTDOWN_DRAIN).await;
        }
        let _ = grace_started_tx.send(());
        let _ = server_shutdown_tx.send(());
    });

    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = server_shutdown_rx.await;
        })
        .into_future();
    tokio::pin!(server);

    let grace_deadline = async move {
        let _ = grace_started_rx.await;
        tokio::time::sleep(shutdown_grace_period).await;
    };

    tokio::select! {
        result = &mut server => {
            result?;
            Ok(())
        }
        _ = grace_deadline => {
            Err(anyhow::anyhow!(
                "graceful shutdown exceeded {}ms",
                shutdown_grace_period.as_millis()
            ))
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C shutdown handler");
    };

    #[cfg(unix)]
    {
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM shutdown handler")
                .recv()
                .await;
        };

        tokio::select! {
            _ = ctrl_c => {}
            _ = terminate => {}
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
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
