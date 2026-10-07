# Operations and troubleshooting

## Metrics and health

`GET /metrics` exposes lightweight Prometheus text metrics for operational visibility:

- `aximo_http_requests_total{status,code}`
- `aximo_errors_total{code}`
- `aximo_audio_body_bytes_total`
- `aximo_audio_decode_seconds_bucket/sum/count`
- `aximo_audio_duration_seconds_bucket/sum/count`
- `aximo_inference_wait_seconds_bucket/sum/count{kind}`
- `aximo_model_execution_wait_seconds_bucket/sum/count{kind}`
- `aximo_inference_seconds_bucket/sum/count{kind}`
- `aximo_rtf_bucket/sum/count{kind}`
- `aximo_inference_timeouts_total{kind}`
- `aximo_blocking_tasks_active`
- `aximo_model_executions_active`
- `aximo_runtime_degraded`
- `aximo_runtime_consecutive_failures`
- `aximo_runtime_component_degraded{component}`
- `aximo_runtime_component_consecutive_failures{component}`
- `aximo_ws_sessions_active`
- `aximo_ws_queue_overflows_total`
- `aximo_realtime_partial_coalesced_total`
- `aximo_realtime_stale_partial_skips_total`
- `aximo_model_execution_wait_timeouts_total`

Latency and RTF metrics are emitted as Prometheus histograms, so dashboards can use `histogram_quantile()` for p95/p99 without depending only on averages.

`/health/live` is process liveness. `/health/ready` reports aggregate readiness and per-component details such as `short:parakeet`, `realtime_partial:parakeet`, and `realtime_final:parakeet`. It returns `503` with a JSON `degraded` status after consecutive timeout/runtime/unavailable inference failures for any component reach `runtime_degrade_after_consecutive_failures`. A successful inference clears only its own component state. `runtime_degraded_policy = "readiness_only"` only signals orchestrators through readiness; `runtime_degraded_policy = "fail_fast_inference"` additionally rejects new inference work for degraded components with `engine_degraded`, then allows one half-open recovery probe after `runtime_degraded_recovery_cooldown_ms`. Client-side errors that stop a half-open probe before inference consume the probe window and restart the cooldown without changing the prior engine failure reason.

On SIGINT or SIGTERM, Aximo notifies active websocket handlers, sends close frames, stops accepting new connections through axum graceful shutdown, and waits up to `shutdown_grace_period_ms`.

## Troubleshooting

If container logs include `onnxruntime cpuid_info warning: Unknown CPU vendor`, this is typically an ONNX Runtime CPU detection warning on ARM or virtualized environments, not a model-load failure. The container now sets `ORT_LOG=error` to reduce that noise in normal runs.

## Known limits

- Realtime uses a native streaming session only when `/v1/capabilities` reports `supports_native_streaming=true`; Parakeet and GigaAM currently report `false`, so they intentionally use bounded buffered realtime.
- Native streaming currently uses one native worker thread per active native streaming session. This keeps backend calls out of the WebSocket loop, but high native-streaming session counts must be benchmarked before raising `max_realtime_sessions`.
- `segments` is backend-dependent and only returned when `timestamps=true`; `detected_language` stays `null` while `/v1/capabilities` reports `supports_language_detection=false`.
- Container decode now avoids an extra input-buffer copy from axum `Bytes`, but decoded samples are still materialized in memory before normalization.
- Audio resampling now uses a bounded windowed-sinc path, but production WER/CER work should still validate preprocessing quality against real audio.
- Remaining product work is backend-driven: plug in a backend that exposes language detection when that capability is required.
