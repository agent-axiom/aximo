use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use std::path::PathBuf;
use tower::ServiceExt;

fn fixture_bytes(name: &str) -> Vec<u8> {
    let fixtures_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../aximo-audio/tests/fixtures");
    std::fs::read(fixtures_dir.join(name)).unwrap()
}

#[tokio::test]
async fn metrics_endpoint_reports_short_audio_observability_series() {
    let app = aximo::app::build_test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcriptions")
                .header("content-type", "audio/wav")
                .body(Body::from(fixture_bytes("tone-16k-mono.wav")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let metrics_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(metrics_response.status(), StatusCode::OK);

    let body = to_bytes(metrics_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let metrics = String::from_utf8(body.to_vec()).unwrap();

    assert!(metrics.contains(r#"aximo_http_requests_total{status="200",code="ok"} 1"#));
    assert!(metrics.contains("aximo_audio_body_bytes_total "));
    assert!(metrics.contains("# HELP aximo_audio_decode_seconds Audio decode time in seconds."));
    assert!(metrics.contains("# TYPE aximo_audio_decode_seconds histogram"));
    assert!(metrics.contains(r#"aximo_audio_decode_seconds_bucket{le="+Inf"} 1"#));
    assert!(metrics.contains("aximo_audio_decode_seconds_sum "));
    assert!(metrics.contains("aximo_audio_decode_seconds_count 1"));
    assert!(metrics.contains(r#"aximo_audio_duration_seconds_bucket{le="+Inf"} 1"#));
    assert!(metrics.contains("aximo_audio_duration_seconds_sum "));
    assert!(metrics.contains("aximo_audio_duration_seconds_count 1"));
    assert!(metrics.contains(r#"aximo_inference_wait_seconds_bucket{kind="short",le="+Inf"} 1"#));
    assert!(
        metrics.contains(r#"aximo_model_execution_wait_seconds_bucket{kind="short",le="+Inf"} 1"#)
    );
    assert!(metrics.contains(r#"aximo_model_execution_wait_seconds_count{kind="short"} 1"#));
    assert!(metrics.contains(r#"aximo_inference_seconds_bucket{kind="short",le="+Inf"} 1"#));
    assert!(metrics.contains(r#"aximo_inference_seconds_sum{kind="short"} "#));
    assert!(metrics.contains(r#"aximo_inference_seconds_count{kind="short"} 1"#));
    assert!(metrics.contains(r#"aximo_rtf_bucket{kind="short",le="+Inf"} 1"#));
    assert!(metrics.contains(r#"aximo_rtf_sum{kind="short"} "#));
    assert!(metrics.contains(r#"aximo_rtf_count{kind="short"} 1"#));
    assert!(metrics.contains("aximo_blocking_tasks_active 0"));
    assert!(metrics.contains("aximo_model_executions_active 0"));
    assert!(metrics.contains("aximo_runtime_degraded 0"));
    assert!(metrics.contains("aximo_runtime_consecutive_failures 0"));
    assert!(metrics.contains(r#"aximo_runtime_component_degraded{component="short:parakeet"} 0"#));
    assert!(metrics
        .contains(r#"aximo_runtime_component_consecutive_failures{component="short:parakeet"} 0"#));
    assert!(metrics.contains("aximo_ws_sessions_active "));
}
