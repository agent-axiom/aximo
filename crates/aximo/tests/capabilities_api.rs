use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

struct NativeCapabilityEngine;

impl aximo_inference::engine::SpeechEngine for NativeCapabilityEngine {
    fn transcribe_short(
        &self,
        _request: aximo_core::ShortAudioRequest,
    ) -> Result<aximo_core::ShortAudioResult, aximo_inference::engine::InferenceError> {
        Ok(aximo_core::ShortAudioResult::new("native", "native-test"))
    }

    fn capabilities(&self) -> aximo_core::EngineCapabilities {
        aximo_core::EngineCapabilities {
            engine: "native-test".to_string(),
            model_name: "Native Test".to_string(),
            sample_rate_hz: 16_000,
            languages: vec!["en".to_string()],
            supports_timestamps: false,
            supports_language_detection: false,
            supports_native_streaming: true,
        }
    }
}

#[tokio::test]
async fn capabilities_endpoint_reports_engine_metadata_and_realtime_mode() {
    let app = aximo::app::build_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/capabilities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["offline"]["configured_engine"], "parakeet");
    assert_eq!(json["offline"]["model"]["engine"], "fake");
    assert_eq!(
        json["offline"]["model"]["languages"],
        serde_json::json!(["en", "ru"])
    );
    assert_eq!(json["offline"]["model"]["supports_timestamps"], true);
    assert_eq!(
        json["offline"]["model"]["supports_language_detection"],
        false
    );
    assert_eq!(json["offline"]["model"]["supports_native_streaming"], false);
    assert_eq!(json["realtime"]["mode"], "bounded_buffered_offline");
}

#[tokio::test]
async fn capabilities_endpoint_reports_native_streaming_mode_when_backend_supports_it() {
    let app = aximo::app::build_app(
        aximo::config::Settings::default(),
        Arc::new(aximo_inference::engine::FakeEngine),
        Arc::new(NativeCapabilityEngine),
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/capabilities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["realtime"]["mode"], "native_streaming");
    assert_eq!(json["realtime"]["model"]["supports_native_streaming"], true);
}
