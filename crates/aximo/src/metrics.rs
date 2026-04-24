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

const LATENCY_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0,
];
const AUDIO_DURATION_BUCKETS: &[f64] = &[1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0];
const RTF_BUCKETS: &[f64] = &[0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0];

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
    audio_decode_seconds_count: u64,
    audio_decode_seconds_buckets: Vec<u64>,
    audio_duration_seconds_sum: f64,
    audio_duration_seconds_count: u64,
    audio_duration_seconds_buckets: Vec<u64>,
    inference_wait_seconds_sum: BTreeMap<&'static str, f64>,
    inference_wait_seconds_count: BTreeMap<&'static str, u64>,
    inference_wait_seconds_buckets: BTreeMap<&'static str, Vec<u64>>,
    model_execution_wait_seconds_sum: BTreeMap<&'static str, f64>,
    model_execution_wait_seconds_count: BTreeMap<&'static str, u64>,
    model_execution_wait_seconds_buckets: BTreeMap<&'static str, Vec<u64>>,
    inference_seconds_sum: BTreeMap<&'static str, f64>,
    inference_seconds_count: BTreeMap<&'static str, u64>,
    inference_seconds_buckets: BTreeMap<&'static str, Vec<u64>>,
    rtf_sum: BTreeMap<&'static str, f64>,
    rtf_count: BTreeMap<&'static str, u64>,
    rtf_buckets: BTreeMap<&'static str, Vec<u64>>,
    inference_timeouts_total: BTreeMap<&'static str, u64>,
    blocking_tasks_active: i64,
    model_executions_active: i64,
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
        state.audio_decode_seconds_count = state.audio_decode_seconds_count.saturating_add(1);
        observe_bucket(
            &mut state.audio_decode_seconds_buckets,
            LATENCY_BUCKETS,
            elapsed.as_secs_f64(),
        );
        if let Some(duration_ms) = duration_ms {
            let duration_seconds = duration_ms as f64 / 1000.0;
            state.audio_duration_seconds_sum += duration_seconds;
            state.audio_duration_seconds_count =
                state.audio_duration_seconds_count.saturating_add(1);
            observe_bucket(
                &mut state.audio_duration_seconds_buckets,
                AUDIO_DURATION_BUCKETS,
                duration_seconds,
            );
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
        *state.inference_wait_seconds_count.entry(kind).or_default() += 1;
        *state.inference_seconds_sum.entry(kind).or_default() += elapsed.as_secs_f64();
        *state.inference_seconds_count.entry(kind).or_default() += 1;
        observe_bucket(
            state
                .inference_wait_seconds_buckets
                .entry(kind)
                .or_default(),
            LATENCY_BUCKETS,
            wait.as_secs_f64(),
        );
        observe_bucket(
            state.inference_seconds_buckets.entry(kind).or_default(),
            LATENCY_BUCKETS,
            elapsed.as_secs_f64(),
        );

        if audio_duration_ms > 0 {
            let audio_seconds = audio_duration_ms as f64 / 1000.0;
            let rtf = elapsed.as_secs_f64() / audio_seconds;
            *state.rtf_sum.entry(kind).or_default() += rtf;
            *state.rtf_count.entry(kind).or_default() += 1;
            observe_bucket(state.rtf_buckets.entry(kind).or_default(), RTF_BUCKETS, rtf);
        }
    }

    pub fn record_model_execution_wait(&self, kind: &'static str, elapsed: Duration) {
        let mut state = self.inner.lock().expect("metrics lock");
        *state
            .model_execution_wait_seconds_sum
            .entry(kind)
            .or_default() += elapsed.as_secs_f64();
        *state
            .model_execution_wait_seconds_count
            .entry(kind)
            .or_default() += 1;
        observe_bucket(
            state
                .model_execution_wait_seconds_buckets
                .entry(kind)
                .or_default(),
            LATENCY_BUCKETS,
            elapsed.as_secs_f64(),
        );
    }

    pub fn record_inference_timeout(&self, kind: &'static str) {
        let mut state = self.inner.lock().expect("metrics lock");
        *state.inference_timeouts_total.entry(kind).or_default() += 1;
    }

    pub fn inc_blocking_tasks_active(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        state.blocking_tasks_active += 1;
    }

    pub fn dec_blocking_tasks_active(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        if state.blocking_tasks_active > 0 {
            state.blocking_tasks_active -= 1;
        }
    }

    pub fn inc_model_executions_active(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        state.model_executions_active += 1;
    }

    pub fn dec_model_executions_active(&self) {
        let mut state = self.inner.lock().expect("metrics lock");
        if state.model_executions_active > 0 {
            state.model_executions_active -= 1;
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

        writeln!(
            output,
            "# HELP aximo_http_requests_total HTTP requests by response status and API code."
        )
        .expect("write metrics");
        writeln!(output, "# TYPE aximo_http_requests_total counter").expect("write metrics");
        for ((status, code), value) in &state.http_requests_total {
            writeln!(
                output,
                "aximo_http_requests_total{{status=\"{status}\",code=\"{code}\"}} {value}"
            )
            .expect("write metrics");
        }

        writeln!(
            output,
            "# HELP aximo_errors_total Errors by API or websocket code."
        )
        .expect("write metrics");
        writeln!(output, "# TYPE aximo_errors_total counter").expect("write metrics");
        for (code, value) in &state.errors_total {
            writeln!(output, "aximo_errors_total{{code=\"{code}\"}} {value}")
                .expect("write metrics");
        }

        writeln!(
            output,
            "# HELP aximo_audio_body_bytes_total Total HTTP short-audio request body bytes."
        )
        .expect("write metrics");
        writeln!(output, "# TYPE aximo_audio_body_bytes_total counter").expect("write metrics");
        writeln!(
            output,
            "aximo_audio_body_bytes_total {}",
            state.audio_body_bytes_total
        )
        .expect("write metrics");
        write_histogram(
            &mut output,
            "aximo_audio_decode_seconds",
            "Audio decode time in seconds.",
            LATENCY_BUCKETS,
            state.audio_decode_seconds_sum,
            state.audio_decode_seconds_count,
            &state.audio_decode_seconds_buckets,
        );
        write_histogram(
            &mut output,
            "aximo_audio_duration_seconds",
            "Decoded audio duration in seconds.",
            AUDIO_DURATION_BUCKETS,
            state.audio_duration_seconds_sum,
            state.audio_duration_seconds_count,
            &state.audio_duration_seconds_buckets,
        );
        write_labelled_histogram(
            &mut output,
            "aximo_inference_wait_seconds",
            "Admission scheduler wait time in seconds.",
            LATENCY_BUCKETS,
            &state.inference_wait_seconds_sum,
            &state.inference_wait_seconds_count,
            &state.inference_wait_seconds_buckets,
        );
        write_labelled_histogram(
            &mut output,
            "aximo_model_execution_wait_seconds",
            "Per-engine execution gate wait time in seconds.",
            LATENCY_BUCKETS,
            &state.model_execution_wait_seconds_sum,
            &state.model_execution_wait_seconds_count,
            &state.model_execution_wait_seconds_buckets,
        );
        write_labelled_histogram(
            &mut output,
            "aximo_inference_seconds",
            "Client-visible inference time in seconds.",
            LATENCY_BUCKETS,
            &state.inference_seconds_sum,
            &state.inference_seconds_count,
            &state.inference_seconds_buckets,
        );
        write_labelled_histogram(
            &mut output,
            "aximo_rtf",
            "Real-time factor measured as inference seconds divided by audio seconds.",
            RTF_BUCKETS,
            &state.rtf_sum,
            &state.rtf_count,
            &state.rtf_buckets,
        );

        writeln!(
            output,
            "# HELP aximo_inference_timeouts_total Inference requests that exceeded configured timeout budgets."
        )
        .expect("write metrics");
        writeln!(output, "# TYPE aximo_inference_timeouts_total counter").expect("write metrics");
        for (kind, value) in &state.inference_timeouts_total {
            writeln!(
                output,
                "aximo_inference_timeouts_total{{kind=\"{kind}\"}} {value}"
            )
            .expect("write metrics");
        }

        writeln!(
            output,
            "# HELP aximo_blocking_tasks_active Blocking inference tasks currently submitted or running."
        )
        .expect("write metrics");
        writeln!(output, "# TYPE aximo_blocking_tasks_active gauge").expect("write metrics");
        writeln!(
            output,
            "aximo_blocking_tasks_active {}",
            state.blocking_tasks_active
        )
        .expect("write metrics");
        writeln!(
            output,
            "# HELP aximo_model_executions_active Model execution gates currently held."
        )
        .expect("write metrics");
        writeln!(output, "# TYPE aximo_model_executions_active gauge").expect("write metrics");
        writeln!(
            output,
            "aximo_model_executions_active {}",
            state.model_executions_active
        )
        .expect("write metrics");

        writeln!(
            output,
            "# HELP aximo_ws_sessions_active Active realtime websocket sessions."
        )
        .expect("write metrics");
        writeln!(output, "# TYPE aximo_ws_sessions_active gauge").expect("write metrics");
        writeln!(
            output,
            "aximo_ws_sessions_active {}",
            state.ws_sessions_active
        )
        .expect("write metrics");
        writeln!(
            output,
            "# HELP aximo_ws_queue_overflows_total Realtime websocket event queue overflows."
        )
        .expect("write metrics");
        writeln!(output, "# TYPE aximo_ws_queue_overflows_total counter").expect("write metrics");
        writeln!(
            output,
            "aximo_ws_queue_overflows_total {}",
            state.ws_queue_overflows_total
        )
        .expect("write metrics");
        writeln!(
            output,
            "# HELP aximo_realtime_partial_coalesced_total Realtime partial inference passes coalesced into a follow-up pass."
        )
        .expect("write metrics");
        writeln!(
            output,
            "# TYPE aximo_realtime_partial_coalesced_total counter"
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
            "# HELP aximo_realtime_stale_partial_skips_total Realtime partial tasks skipped because the session disappeared."
        )
        .expect("write metrics");
        writeln!(
            output,
            "# TYPE aximo_realtime_stale_partial_skips_total counter"
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

fn observe_bucket(bucket_counts: &mut Vec<u64>, boundaries: &[f64], value: f64) {
    if bucket_counts.len() < boundaries.len() {
        bucket_counts.resize(boundaries.len(), 0);
    }

    for (index, boundary) in boundaries.iter().enumerate() {
        if value <= *boundary {
            bucket_counts[index] = bucket_counts[index].saturating_add(1);
        }
    }
}

fn write_histogram(
    output: &mut String,
    name: &str,
    help: &str,
    boundaries: &[f64],
    sum: f64,
    count: u64,
    bucket_counts: &[u64],
) {
    writeln!(output, "# HELP {name} {help}").expect("write metrics");
    writeln!(output, "# TYPE {name} histogram").expect("write metrics");
    for (index, boundary) in boundaries.iter().enumerate() {
        writeln!(
            output,
            r#"{name}_bucket{{le="{}"}} {}"#,
            format_bucket(*boundary),
            bucket_counts.get(index).copied().unwrap_or(0)
        )
        .expect("write metrics");
    }
    writeln!(output, r#"{name}_bucket{{le="+Inf"}} {count}"#).expect("write metrics");
    writeln!(output, "{name}_sum {sum:.9}").expect("write metrics");
    writeln!(output, "{name}_count {count}").expect("write metrics");
}

fn write_labelled_histogram(
    output: &mut String,
    name: &str,
    help: &str,
    boundaries: &[f64],
    sums: &BTreeMap<&'static str, f64>,
    counts: &BTreeMap<&'static str, u64>,
    buckets: &BTreeMap<&'static str, Vec<u64>>,
) {
    writeln!(output, "# HELP {name} {help}").expect("write metrics");
    writeln!(output, "# TYPE {name} histogram").expect("write metrics");
    for (kind, count) in counts {
        let bucket_counts = buckets.get(kind).map_or(&[][..], Vec::as_slice);
        for (index, boundary) in boundaries.iter().enumerate() {
            writeln!(
                output,
                r#"{name}_bucket{{kind="{kind}",le="{}"}} {}"#,
                format_bucket(*boundary),
                bucket_counts.get(index).copied().unwrap_or(0)
            )
            .expect("write metrics");
        }
        writeln!(
            output,
            r#"{name}_bucket{{kind="{kind}",le="+Inf"}} {count}"#
        )
        .expect("write metrics");
        writeln!(
            output,
            r#"{name}_sum{{kind="{kind}"}} {:.9}"#,
            sums.get(kind).copied().unwrap_or_default()
        )
        .expect("write metrics");
        writeln!(output, r#"{name}_count{{kind="{kind}"}} {count}"#).expect("write metrics");
    }
}

fn format_bucket(boundary: f64) -> String {
    let formatted = boundary.to_string();
    formatted
        .strip_suffix(".0")
        .unwrap_or(&formatted)
        .to_string()
}

pub async fn metrics(State(state): State<AppState>) -> Response {
    let mut body = state.metrics.render_prometheus();
    let readiness = state.runtime_health.readiness();
    writeln!(
        body,
        "# HELP aximo_runtime_degraded Runtime readiness degraded state."
    )
    .expect("write metrics");
    writeln!(body, "# TYPE aximo_runtime_degraded gauge").expect("write metrics");
    writeln!(
        body,
        "aximo_runtime_degraded {}",
        u8::from(readiness.status == "degraded")
    )
    .expect("write metrics");
    writeln!(
        body,
        "# HELP aximo_runtime_consecutive_failures Consecutive runtime inference failures tracked by readiness."
    )
    .expect("write metrics");
    writeln!(body, "# TYPE aximo_runtime_consecutive_failures gauge").expect("write metrics");
    writeln!(
        body,
        "aximo_runtime_consecutive_failures {}",
        readiness.consecutive_failures
    )
    .expect("write metrics");
    writeln!(
        body,
        "# HELP aximo_runtime_component_degraded Runtime readiness degraded state by component."
    )
    .expect("write metrics");
    writeln!(body, "# TYPE aximo_runtime_component_degraded gauge").expect("write metrics");
    for component in &readiness.components {
        writeln!(
            body,
            "aximo_runtime_component_degraded{{component=\"{}\"}} {}",
            component.component,
            u8::from(component.status == "degraded")
        )
        .expect("write metrics");
    }
    writeln!(
        body,
        "# HELP aximo_runtime_component_consecutive_failures Consecutive runtime inference failures by component."
    )
    .expect("write metrics");
    writeln!(
        body,
        "# TYPE aximo_runtime_component_consecutive_failures gauge"
    )
    .expect("write metrics");
    for component in &readiness.components {
        writeln!(
            body,
            "aximo_runtime_component_consecutive_failures{{component=\"{}\"}} {}",
            component.component, component.consecutive_failures
        )
        .expect("write metrics");
    }

    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}
