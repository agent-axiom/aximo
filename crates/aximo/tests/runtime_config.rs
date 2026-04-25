use std::{fs, path::PathBuf};

use aximo::{
    config::{RuntimeDegradedPolicy, Settings},
    runtime::{resolve_engine_spec, RuntimeConfigError},
};
use aximo_inference::runtime::EngineKind;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[test]
fn settings_can_be_loaded_from_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aximo.toml");
    fs::write(
        &path,
        r#"
[server]
host = "127.0.0.1"
port = 9090
shutdown_grace_period_ms = 1500

[inference]
models_dir = "/srv/models"
default_offline_engine = "gigaam"
default_realtime_engine = "parakeet"

[inference.engines.parakeet]
kind = "parakeet"
path = "parakeet-tdt-0.6b-v3-int8"

[inference.engines.gigaam]
kind = "gigaam"
path = "giga-am-v3"

[limits]
max_short_audio_requests = 4
max_short_audio_bytes = 12000000
max_short_raw_pcm_bytes = 960000
max_short_audio_duration_ms = 30000
max_short_decoded_samples = 2880000
max_realtime_sessions = 2
max_short_inferences = 1
max_realtime_inferences = 1
max_realtime_session_bytes = 960000
max_realtime_session_duration_ms = 30000
realtime_partial_min_interval_ms = 450
realtime_partial_min_chunk_bytes = 12000
realtime_event_channel_capacity = 32
short_inference_timeout_ms = 90000
realtime_partial_timeout_ms = 4000
realtime_final_timeout_ms = 95000
runtime_degrade_after_consecutive_failures = 6
runtime_degraded_policy = "fail_fast_inference"
"#,
    )
    .unwrap();

    let settings = Settings::from_path(&path).unwrap();

    assert_eq!(settings.server.host, "127.0.0.1");
    assert_eq!(settings.server.port, 9090);
    assert_eq!(settings.server.shutdown_grace_period_ms, 1500);
    assert_eq!(settings.inference.models_dir, "/srv/models");
    assert_eq!(settings.limits.max_short_audio_bytes, 12000000);
    assert_eq!(settings.limits.max_short_raw_pcm_bytes, 960000);
    assert_eq!(settings.limits.max_short_audio_duration_ms, 30000);
    assert_eq!(settings.limits.max_short_decoded_samples, 2880000);
    assert_eq!(settings.limits.max_realtime_sessions, 2);
    assert_eq!(settings.limits.max_short_inferences, 1);
    assert_eq!(settings.limits.max_realtime_inferences, 1);
    assert_eq!(settings.limits.max_realtime_session_bytes, 960000);
    assert_eq!(settings.limits.max_realtime_session_duration_ms, 30000);
    assert_eq!(settings.limits.realtime_partial_min_interval_ms, 450);
    assert_eq!(settings.limits.realtime_partial_min_chunk_bytes, 12000);
    assert_eq!(settings.limits.realtime_event_channel_capacity, 32);
    assert_eq!(settings.limits.short_inference_timeout_ms, 90000);
    assert_eq!(settings.limits.realtime_partial_timeout_ms, 4000);
    assert_eq!(settings.limits.realtime_final_timeout_ms, 95000);
    assert_eq!(
        settings.limits.runtime_degrade_after_consecutive_failures,
        6
    );
    assert_eq!(
        settings.limits.runtime_degraded_policy,
        RuntimeDegradedPolicy::FailFastInference
    );
}

#[test]
fn runtime_resolution_builds_engine_spec_from_settings() {
    let settings = Settings::default();

    let spec = resolve_engine_spec(&settings, "gigaam").unwrap();

    assert_eq!(spec.kind, EngineKind::Gigaam);
    assert_eq!(
        spec.model_path,
        PathBuf::from("/var/lib/aximo/models").join("giga-am-v3")
    );
}

#[test]
fn runtime_resolution_rejects_unknown_engine_name() {
    let settings = Settings::default();

    let error = resolve_engine_spec(&settings, "missing").unwrap_err();

    assert!(matches!(error, RuntimeConfigError::MissingEngine(_)));
}

#[test]
fn runtime_loader_returns_error_for_missing_model_path() {
    let settings = Settings::default();

    let error = match aximo::runtime::load_engine(&settings, "parakeet") {
        Ok(_) => panic!("expected load_engine to fail without a local model directory"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("does not exist"));
}

#[test]
fn settings_fill_in_default_inference_limits_when_config_omits_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aximo.toml");
    fs::write(
        &path,
        r#"
[server]
host = "127.0.0.1"
port = 9090

[inference]
models_dir = "/srv/models"
default_offline_engine = "gigaam"
default_realtime_engine = "parakeet"

[inference.engines.parakeet]
kind = "parakeet"
path = "parakeet-tdt-0.6b-v3-int8"

[inference.engines.gigaam]
kind = "gigaam"
path = "giga-am-v3"

[limits]
max_short_audio_requests = 4
max_realtime_sessions = 2
"#,
    )
    .unwrap();

    let settings = Settings::from_path(&path).unwrap();

    assert_eq!(settings.limits.max_short_inferences, 1);
    assert_eq!(settings.limits.max_short_audio_bytes, 25_000_000);
    assert_eq!(settings.limits.max_short_raw_pcm_bytes, 1_920_000);
    assert_eq!(settings.limits.max_short_audio_duration_ms, 60_000);
    assert_eq!(settings.limits.max_short_decoded_samples, 5_760_000);
    assert_eq!(settings.limits.max_realtime_inferences, 1);
    assert_eq!(settings.limits.max_realtime_session_bytes, 1_920_000);
    assert_eq!(settings.limits.max_realtime_session_duration_ms, 60_000);
    assert_eq!(settings.limits.realtime_partial_min_interval_ms, 300);
    assert_eq!(settings.limits.realtime_partial_min_chunk_bytes, 9_600);
    assert_eq!(settings.limits.realtime_event_channel_capacity, 64);
    assert_eq!(settings.limits.short_inference_timeout_ms, 120_000);
    assert_eq!(settings.limits.realtime_partial_timeout_ms, 5_000);
    assert_eq!(settings.limits.realtime_final_timeout_ms, 120_000);
    assert_eq!(
        settings.limits.runtime_degrade_after_consecutive_failures,
        3
    );
}

#[tokio::test]
async fn runtime_server_exits_after_shutdown_signal() {
    let app = aximo::app::build_test_app().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(aximo::runtime::serve_with_shutdown(
        listener,
        app,
        async move {
            let _ = shutdown_rx.await;
        },
        std::time::Duration::from_millis(500),
    ));

    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn runtime_server_stops_accepting_new_connections_after_shutdown_signal() {
    let (app, app_shutdown) = aximo::app::build_test_app_with_shutdown().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(aximo::runtime::serve_with_shutdown_notifying_app(
        listener,
        app,
        async move {
            let _ = shutdown_rx.await;
        },
        std::time::Duration::from_secs(2),
        app_shutdown,
    ));

    let response = raw_http_get(address, "/health/live").await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));

    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn runtime_shutdown_drains_active_websocket_before_grace_deadline() {
    let (app, app_shutdown) = aximo::app::build_test_app_with_shutdown().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(aximo::runtime::serve_with_shutdown_notifying_app(
        listener,
        app,
        async move {
            let _ = shutdown_rx.await;
        },
        std::time::Duration::from_millis(25),
        app_shutdown,
    ));
    let (mut socket, _) = connect_async(format!("ws://{address}/v1/realtime"))
        .await
        .unwrap();
    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await
        .unwrap();
    let started = socket.next().await.unwrap().unwrap();
    assert!(started.is_text());

    shutdown_tx.send(()).unwrap();
    let close = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
        .await
        .unwrap();
    match close {
        None => {}
        Some(Ok(message)) => assert!(message.is_close()),
        Some(Err(error)) => assert!(error
            .to_string()
            .contains("reset without closing handshake")),
    }

    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

async fn raw_http_get(address: std::net::SocketAddr, path: &str) -> std::io::Result<String> {
    let mut stream = tokio::net::TcpStream::connect(address).await?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;

    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}
