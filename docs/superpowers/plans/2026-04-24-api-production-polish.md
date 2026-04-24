# API Production Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tighten Aximo's external API contract and operational behavior around limits, MIME parsing, inference timeouts, and observability.

**Architecture:** Keep the existing Rust workspace structure and add small focused units: strict audio media-type parsing in `aximo-audio`, timeout-aware inference helpers in `aximo`, and lightweight in-process Prometheus text metrics. Avoid changing the STT backend contract until `transcribe-rs` exposes segments/language metadata.

**Tech Stack:** Rust 1.88, axum, tokio, tower tests, in-process atomics for metrics, existing `cargo fmt`, `clippy`, `cargo test`, and `cargo llvm-cov` gates.

---

### Task 1: Uniform Short-Audio Error Contract

**Files:**
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo/tests/transcriptions_api.rs`

- [ ] Add an integration test that exceeds `DefaultBodyLimit` and asserts a JSON body with `code = "payload_too_large"`.
- [ ] Change the handler body extractor to accept `Result<Bytes, BytesRejection>` so extractor rejections are mapped into `HttpError`.
- [ ] Keep audio-layer limit errors mapped to the same `413 payload_too_large` contract.
- [ ] Run `cargo test -p aximo --test transcriptions_api transcription_endpoint_returns_payload_too_large_for_http_body_limit`.
- [ ] Commit as `Unify short-audio body limit errors`.

### Task 2: Strict MIME Parsing

**Files:**
- Create: `crates/aximo-audio/src/media_type.rs`
- Modify: `crates/aximo-audio/src/lib.rs`
- Modify: `crates/aximo-audio/src/decode.rs`
- Modify: `crates/aximo-audio/src/normalize.rs`
- Modify: `crates/aximo-inference/src/runtime.rs`
- Modify: `crates/aximo/tests/transcriptions_api.rs`

- [ ] Add parser tests for `audio/wav; codecs=...`, `audio/x-wav`, `audio/mpeg`, bogus content types, empty strings, and missing HTTP content type.
- [ ] Replace substring matching with an explicit allowlist of normalized media types.
- [ ] Preserve accepted aliases: WAV, MP3/MPEG, FLAC, M4A/MP4/AAC, raw PCM, and octet-stream.
- [ ] Run `cargo test -p aximo-audio && cargo test -p aximo --test transcriptions_api`.
- [ ] Commit as `Parse short-audio media types strictly`.

### Task 3: Inference Timeouts

**Files:**
- Modify: `crates/aximo/src/config.rs`
- Modify: `crates/aximo/src/app.rs`
- Modify: `crates/aximo/src/inference_task.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo/src/ws/handler.rs`
- Modify: `crates/aximo/tests/transcriptions_api.rs`
- Modify: `crates/aximo/tests/realtime_protocol.rs`
- Modify: `config/aximo.example.toml`
- Modify: `config/aximo.local.toml`
- Modify: `README.md`

- [ ] Add config/env fields for `short_inference_timeout_ms`, `realtime_partial_timeout_ms`, and `realtime_final_timeout_ms`.
- [ ] Add tests proving short requests return `504 inference_timeout` when the blocking backend exceeds the timeout.
- [ ] Add realtime tests proving partial and final paths emit structured timeout errors.
- [ ] Implement timeout wrapping around the awaited blocking task. Document that the blocking OS thread may continue, but permits are released when the server stops awaiting it.
- [ ] Run targeted timeout tests.
- [ ] Commit as `Add inference timeout budgets`.

### Task 4: Operational Metrics and WS Overflow Behavior

**Files:**
- Create: `crates/aximo/src/metrics.rs`
- Modify: `crates/aximo/src/lib.rs`
- Modify: `crates/aximo/src/app.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo/src/ws/handler.rs`
- Modify: `crates/aximo/tests/docs_api.rs`
- Modify: `crates/aximo/tests/health_api.rs` or create `crates/aximo/tests/metrics_api.rs`
- Modify: `README.md`

- [ ] Add `/metrics` returning Prometheus text exposition.
- [ ] Track request totals by status/code, audio body bytes, decoded duration, inference wait, inference duration, RTF, active websocket sessions, queue overflows, and stale partial skips.
- [ ] On websocket queue overflow, attempt one final structured error event and then close the connection.
- [ ] Add tests for metrics endpoint output and overflow counter behavior.
- [ ] Run `cargo test -p aximo --test metrics_api` plus websocket targeted tests.
- [ ] Commit as `Expose operational metrics`.

### Task 5: Documentation and Release Notes

**Files:**
- Modify: `README.md`
- Modify: `docs/architecture.md`
- Modify: `docs/realtime-protocol.md`
- Optional create: `deploy/k8s/aximo.yaml`

- [ ] Document shared-engine semaphore semantics and that deduped offline/realtime model instances still serialize through the backend runner.
- [ ] Keep `segments` and `detected_language` documented as backend-limited, not hidden.
- [ ] Add a deployment note for GHCR/versioned tags as a future release step unless a publishing token and repository policy are configured.
- [ ] Run docs/API tests.
- [ ] Commit as `Document production API semantics`.
