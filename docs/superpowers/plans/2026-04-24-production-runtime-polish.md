# Production Runtime Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tighten Aximo's production runtime behavior around health isolation, overflow behavior, observability, short-audio request options, and decode memory use.

**Architecture:** Keep the existing axum/Rust workspace structure. Replace global runtime health with per-component health keyed by `short:<engine>`, `realtime_partial:<engine>`, and `realtime_final:<engine>`, while preserving aggregate readiness. Add fixed Prometheus histogram buckets without pulling in a full metrics registry yet. Keep backend capabilities honest: request options are accepted and forwarded, but timestamps/segments remain backend-dependent.

**Tech Stack:** Rust 1.88, axum, tokio, serde query extraction, existing in-process Prometheus text renderer, existing `cargo fmt`, `clippy`, `cargo test`, and `cargo llvm-cov` gates.

---

### Task 1: Per-Component Runtime Health

**Files:**
- Modify: `crates/aximo/src/runtime_health.rs`
- Modify: `crates/aximo/src/app.rs`
- Modify: `crates/aximo/src/http/health.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo/src/ws/handler.rs`
- Modify: `crates/aximo/src/metrics.rs`
- Test: `crates/aximo/tests/health_api.rs`
- Test: `crates/aximo/tests/metrics_api.rs`

- [x] Write failing health tests proving `short:parakeet` degradation does not mark `realtime_partial:parakeet` degraded and a realtime success does not clear short failures.
- [x] Change `RuntimeHealthState` to store per-component state in a `BTreeMap<String, ComponentState>`.
- [x] Add `record_success(component)` and `record_failure(component, reason)`.
- [x] Add engine names to `AppState` from `Settings.inference.default_*_engine`.
- [x] Record short/realtime health using component keys `short:<engine>`, `realtime_partial:<engine>`, and `realtime_final:<engine>`.
- [x] Render readiness JSON with aggregate status plus component details.
- [x] Render per-component runtime health metrics.
- [x] Run targeted health and metrics tests.
- [x] Commit as `Track runtime health per engine path`.

### Task 2: Integration Regression Tests For Critical Runtime Guarantees

**Files:**
- Modify: `crates/aximo/tests/transcriptions_api.rs`
- Modify: `crates/aximo/tests/realtime_protocol.rs`

- [x] Add HTTP integration test proving a timed-out short request keeps the model gate occupied until its backend call returns.
- [x] Add websocket integration test for bounded event queue overflow/disconnect behavior with `realtime_event_channel_capacity = 1`.
- [x] Run targeted integration tests.
- [x] Commit as `Add runtime backpressure regression tests`.

### Task 3: Histogram Buckets For SLO-Friendly Metrics

**Files:**
- Modify: `crates/aximo/src/metrics.rs`
- Test: `crates/aximo/tests/metrics_api.rs`

- [x] Add fixed histogram buckets for decode seconds, audio duration seconds, scheduler wait seconds, model execution wait seconds, inference seconds, and RTF.
- [x] Keep existing `_sum` and `_count` series for compatibility.
- [x] Emit Prometheus `_bucket{le="..."}` and `_bucket{le="+Inf"}` series.
- [x] Run metrics tests.
- [x] Commit as `Add Prometheus histogram buckets`.

### Task 4: Short-Audio Request Options

**Files:**
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo/src/docs.rs`
- Modify: `README.md`
- Test: `crates/aximo/tests/transcriptions_api.rs`
- Test: `crates/aximo/tests/docs_api.rs`

- [x] Add failing test proving `?language=ru&timestamps=true&engine=parakeet` is forwarded into `ShortAudioRequest`.
- [x] Add failing test proving an unsupported `engine` query value returns structured `400 unsupported_engine`.
- [x] Parse `engine`, `language`, `language_hint`, and `timestamps` from query params.
- [x] Forward options into `ShortAudioRequest`.
- [x] Document query options in OpenAPI and README.
- [x] Run targeted API/docs tests.
- [x] Commit as `Accept short transcription request options`.

### Task 5: Zero-Copy HTTP Decode Source

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/aximo-audio/Cargo.toml`
- Modify: `crates/aximo-audio/src/decode.rs`
- Modify: `crates/aximo-audio/src/lib.rs`
- Modify: `crates/aximo-audio/src/normalize.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Test: `crates/aximo-audio/src/decode.rs`
- Test: `crates/aximo-audio/src/normalize.rs`

- [x] Add `bytes` as a direct workspace dependency.
- [x] Add `decode_container_bytes_with_sample_limit(Bytes, ...)`.
- [x] Add `prepare_short_audio_bytes_with_limits(Bytes, ...)` so axum `Bytes` can be passed without `to_vec()` for container decode.
- [x] Keep existing `&[u8]` APIs for crate consumers.
- [x] Route HTTP short-audio through the Bytes-aware API.
- [x] Run audio and short transcription tests.
- [x] Commit as `Decode short audio from shared bytes`.

### Task 6: Documentation And Future Work

**Files:**
- Modify: `README.md`
- Modify: `docs/architecture.md`
- Modify: `docs/realtime-protocol.md`
- Modify: `docs/superpowers/plans/2026-04-24-production-runtime-polish.md`

- [x] Document per-component readiness semantics.
- [x] Document histogram metrics and request options.
- [x] Keep benchmark suite, higher-quality resampling, timestamps/language backend metadata, audit/deny/SBOM, and streaming decoder as explicit future work.
- [x] Mark plan tasks complete.
- [x] Run docs/API tests.
- [x] Commit as `Document production runtime polish`.
