use std::sync::Arc;

use aximo_audio::ShortAudioLimits;
use aximo_core::{RealtimePartialLimits, RealtimeSessionLimits, Scheduler, SessionManager};
use aximo_inference::engine::{FakeEngine, SpeechEngine};
use axum::{extract::DefaultBodyLimit, Router};

use crate::{
    config::Settings, engine_runtime::EngineRuntime, http, metrics::Metrics,
    runtime_health::RuntimeHealth, ws,
};

#[derive(Clone)]
pub struct AppState {
    pub offline_engine: EngineRuntime,
    pub realtime_engine: EngineRuntime,
    pub session_manager: SessionManager,
    pub scheduler: Scheduler,
    pub short_audio_limits: ShortAudioLimits,
    pub realtime_session_limits: RealtimeSessionLimits,
    pub realtime_partial_limits: RealtimePartialLimits,
    pub realtime_event_channel_capacity: usize,
    pub short_inference_timeout: std::time::Duration,
    pub realtime_partial_timeout: std::time::Duration,
    pub realtime_final_timeout: std::time::Duration,
    pub metrics: Metrics,
    pub runtime_health: RuntimeHealth,
}

pub fn build_app(
    settings: Settings,
    offline_engine: Arc<dyn SpeechEngine>,
    realtime_engine: Arc<dyn SpeechEngine>,
) -> Router {
    let short_audio_body_limit = settings.limits.max_short_audio_bytes;
    let offline_gate = EngineRuntime::shared_gate();
    let realtime_gate = if Arc::ptr_eq(&offline_engine, &realtime_engine) {
        Arc::clone(&offline_gate)
    } else {
        EngineRuntime::shared_gate()
    };
    let state = AppState {
        offline_engine: EngineRuntime::with_gate(offline_engine, offline_gate),
        realtime_engine: EngineRuntime::with_gate(realtime_engine, realtime_gate),
        session_manager: SessionManager::new(),
        scheduler: Scheduler::new(
            settings.limits.max_short_audio_requests,
            settings.limits.max_realtime_sessions,
            settings.limits.max_short_inferences,
            settings.limits.max_realtime_inferences,
        ),
        short_audio_limits: ShortAudioLimits {
            max_raw_pcm_bytes: settings.limits.max_short_raw_pcm_bytes,
            max_duration_ms: settings.limits.max_short_audio_duration_ms,
            max_decoded_samples: settings.limits.max_short_decoded_samples,
        },
        realtime_session_limits: RealtimeSessionLimits {
            max_bytes: settings.limits.max_realtime_session_bytes,
            max_duration: std::time::Duration::from_millis(
                settings.limits.max_realtime_session_duration_ms,
            ),
        },
        realtime_partial_limits: RealtimePartialLimits {
            min_interval: std::time::Duration::from_millis(
                settings.limits.realtime_partial_min_interval_ms,
            ),
            min_chunk_bytes: settings.limits.realtime_partial_min_chunk_bytes,
        },
        realtime_event_channel_capacity: settings.limits.realtime_event_channel_capacity.max(1),
        short_inference_timeout: std::time::Duration::from_millis(
            settings.limits.short_inference_timeout_ms,
        ),
        realtime_partial_timeout: std::time::Duration::from_millis(
            settings.limits.realtime_partial_timeout_ms,
        ),
        realtime_final_timeout: std::time::Duration::from_millis(
            settings.limits.realtime_final_timeout_ms,
        ),
        metrics: Metrics::default(),
        runtime_health: RuntimeHealth::new(
            settings.limits.runtime_degrade_after_consecutive_failures,
        ),
    };

    Router::new()
        .route("/health/live", axum::routing::get(http::health::live))
        .route("/health/ready", axum::routing::get(http::health::ready))
        .route(
            "/v1/transcriptions",
            axum::routing::post(http::transcriptions::transcribe_short)
                .layer(DefaultBodyLimit::max(short_audio_body_limit)),
        )
        .route(
            "/v1/realtime",
            axum::routing::get(ws::handler::realtime_socket),
        )
        .route("/metrics", axum::routing::get(crate::metrics::metrics))
        .merge(crate::docs::router())
        .with_state(state)
}

pub async fn build_test_app() -> Router {
    build_app(
        Settings::default(),
        Arc::new(FakeEngine),
        Arc::new(FakeEngine),
    )
}
