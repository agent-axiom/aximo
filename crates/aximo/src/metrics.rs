use std::{
    collections::BTreeMap,
    fmt::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};

use crate::app::AppState;

#[derive(Clone, Default)]
pub struct Metrics {
    inner: Arc<Mutex<MetricsState>>,
}

#[derive(Default)]
struct MetricsState {
    http_requests_total: BTreeMap<(u16, String), u64>,
    errors_total: BTreeMap<String, u64>,
    audio_body_bytes_total: u64,
    audio_decode_seconds_sum: f64,
    audio_duration_seconds_sum: f64,
    inference_wait_seconds_sum: BTreeMap<&'static str, f64>,
    inference_seconds_sum: BTreeMap<&'static str, f64>,
    rtf_sum: BTreeMap<&'static str, f64>,
    ws_sessions_active: i64,
    ws_queue_overflows_total: u64,
    realtime_partial_coalesced_total: u64,
    realtime_stale_partial_skips_total: u64,
}

impl Metrics {
    pub fn record_http_response(&self, status: u16, code: impl Into<String>) {
        let mut state = self.inner.lock().expect("metrics lock");
        *state
            .http_requests_total
            .entry((status, code.into()))
            .or_default() += 1;
    }

    pub fn record_error(&self, code: impl Into<String>) {
        let mut state = self.inner.lock().expect("metrics lock");
        *state.errors_total.entry(code.into()).or_default() += 1;
    }

    pub fn record_audio_body_bytes(&self, bytes: usize) {
        let mut state = self.inner.lock().expect("metrics lock");
        state.audio_body_bytes_total = state.audio_body_bytes_total.saturating_add(bytes as u64);
    }

    pub fn record_audio_decode(&self, elapsed: Duration, duration_ms: Option<u64>) {
        let mut state = self.inner.lock().expect("metrics lock");
        state.audio_decode_seconds_sum += elapsed.as_secs_f64();
        if let Some(duration_ms) = duration_ms {
            state.audio_duration_seconds_sum += duration_ms as f64 / 1000.0;
        }
    }

    pub fn record_inference(
        &self,
        kind: &'static str,
        wait: Duration,
        elapsed: Duration,
        audio_duration_ms: u64,
    ) {
        let mut state = self.inner.lock().expect("metrics lock");
        *state.inference_wait_seconds_sum.entry(kind).or_default() += wait.as_secs_f64();
        *state.inference_seconds_sum.entry(kind).or_default() += elapsed.as_secs_f64();

        if audio_duration_ms > 0 {
            let audio_seconds = audio_duration_ms as f64 / 1000.0;
            *state.rtf_sum.entry(kind).or_default() += elapsed.as_secs_f64() / audio_seconds;
        }
    }

    pub fn inc_ws_sessions_active(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        state.ws_sessions_active += 1;
    }

    pub fn dec_ws_sessions_active(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        if state.ws_sessions_active > 0 {
            state.ws_sessions_active -= 1;
        }
    }

    pub fn record_ws_queue_overflow(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        state.ws_queue_overflows_total = state.ws_queue_overflows_total.saturating_add(1);
    }

    pub fn record_realtime_partial_coalesced(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        state.realtime_partial_coalesced_total =
            state.realtime_partial_coalesced_total.saturating_add(1);
    }

    pub fn record_realtime_stale_partial_skip(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        state.realtime_stale_partial_skips_total =
            state.realtime_stale_partial_skips_total.saturating_add(1);
    }

    pub fn render_prometheus(&self) -> String {
        let state = self.inner.lock().expect("metrics lock");
        let mut output = String::new();

        for ((status, code), value) in &state.http_requests_total {
            writeln!(
                output,
                "aximo_http_requests_total{{status=\"{status}\",code=\"{code}\"}} {value}"
            )
            .expect("write metrics");
        }

        for (code, value) in &state.errors_total {
            writeln!(output, "aximo_errors_total{{code=\"{code}\"}} {value}")
                .expect("write metrics");
        }

        writeln!(
            output,
            "aximo_audio_body_bytes_total {}",
            state.audio_body_bytes_total
        )
        .expect("write metrics");
        writeln!(
            output,
            "aximo_audio_decode_seconds_sum {:.9}",
            state.audio_decode_seconds_sum
        )
        .expect("write metrics");
        writeln!(
            output,
            "aximo_audio_duration_seconds_sum {:.9}",
            state.audio_duration_seconds_sum
        )
        .expect("write metrics");

        for (kind, value) in &state.inference_wait_seconds_sum {
            writeln!(
                output,
                "aximo_inference_wait_seconds_sum{{kind=\"{kind}\"}} {value:.9}"
            )
            .expect("write metrics");
        }

        for (kind, value) in &state.inference_seconds_sum {
            writeln!(
                output,
                "aximo_inference_seconds_sum{{kind=\"{kind}\"}} {value:.9}"
            )
            .expect("write metrics");
        }

        for (kind, value) in &state.rtf_sum {
            writeln!(output, "aximo_rtf_sum{{kind=\"{kind}\"}} {value:.9}")
                .expect("write metrics");
        }

        writeln!(
            output,
            "aximo_ws_sessions_active {}",
            state.ws_sessions_active
        )
        .expect("write metrics");
        writeln!(
            output,
            "aximo_ws_queue_overflows_total {}",
            state.ws_queue_overflows_total
        )
        .expect("write metrics");
        writeln!(
            output,
            "aximo_realtime_partial_coalesced_total {}",
            state.realtime_partial_coalesced_total
        )
        .expect("write metrics");
        writeln!(
            output,
            "aximo_realtime_stale_partial_skips_total {}",
            state.realtime_stale_partial_skips_total
        )
        .expect("write metrics");

        output
    }
}

pub async fn metrics(State(state): State<AppState>) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render_prometheus(),
    )
        .into_response()
}
