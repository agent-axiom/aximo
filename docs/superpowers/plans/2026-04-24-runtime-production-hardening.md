# Runtime Production Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Aximo's runtime limits, health, metrics, and websocket input validation more honest under production failure modes.

**Architecture:** Keep request admission limits in `Scheduler`, but add a per-engine execution gate that is shared when offline and realtime use the same engine `Arc`. The gate is acquired before spawning blocking work and moved into the blocking closure, so a client timeout does not release model execution capacity before the backend call really exits. Health/readiness and metrics read shared runtime health state.

**Tech Stack:** Rust 1.88, axum, tokio semaphores, existing in-process Prometheus text metrics, existing `cargo fmt`, `clippy`, `cargo test`, and `cargo llvm-cov` gates.

---

### Task 1: Model-Level Execution Gate

**Files:**
- Create: `crates/aximo/src/engine_runtime.rs`
- Modify: `crates/aximo/src/lib.rs`
- Modify: `crates/aximo/src/app.rs`
- Modify: `crates/aximo/src/inference_task.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo/src/ws/handler.rs`
- Test: `crates/aximo/tests/blocking_inference.rs`
- Test: `crates/aximo/tests/transcriptions_api.rs`

- [ ] Write a test proving two timed-out short requests against one shared engine cannot both enter backend execution while the first backend call is still running.
- [ ] Add `EngineRuntime` with an `Arc<dyn SpeechEngine>` and an `Arc<Semaphore>`.
- [ ] Build offline/realtime runtimes with one shared gate when both engine `Arc`s are pointer-equal.
- [ ] Change blocking inference helper to acquire the model permit before `spawn_blocking` and move the permit into the blocking closure.
- [ ] Run targeted timeout/concurrency tests.
- [ ] Commit as `Hold model execution permits through blocking calls`.

### Task 2: Metrics Upgrade

**Files:**
- Modify: `crates/aximo/src/metrics.rs`
- Modify: `crates/aximo/src/inference_task.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo/src/ws/handler.rs`
- Test: `crates/aximo/tests/metrics_api.rs`

- [ ] Add `_count` metrics for decode, audio duration, inference wait, inference duration, and RTF.
- [ ] Emit Prometheus `# HELP` and `# TYPE` lines for every metric family.
- [ ] Track active blocking tasks and active engine executions.
- [ ] Record model execution wait separately from scheduler admission wait.
- [ ] Run `cargo test -p aximo --test metrics_api`.
- [ ] Commit as `Improve runtime metrics fidelity`.

### Task 3: Readiness and Runtime Health

**Files:**
- Create: `crates/aximo/src/health.rs`
- Modify: `crates/aximo/src/app.rs`
- Modify: `crates/aximo/src/http/health.rs`
- Test: `crates/aximo/tests/health_api.rs`

- [ ] Add `/health/live` as process liveness.
- [ ] Make `/health/ready` read runtime health state.
- [ ] Mark health degraded when repeated inference timeouts or runtime failures cross a small threshold.
- [ ] Return `503` while degraded and expose the degraded state in metrics.
- [ ] Run `cargo test -p aximo --test health_api`.
- [ ] Commit as `Make readiness reflect runtime health`.

### Task 4: Realtime PCM Alignment

**Files:**
- Modify: `crates/aximo/src/ws/handler.rs`
- Test: `crates/aximo/tests/realtime_protocol.rs`
- Modify: `docs/realtime-protocol.md`

- [ ] Add a websocket regression test that sends an odd-length binary frame and expects `invalid_audio`.
- [ ] Reject odd-length realtime PCM chunks before appending to the session buffer.
- [ ] Document that every binary chunk must be aligned to `pcm_s16le` sample boundaries.
- [ ] Run `cargo test -p aximo --test realtime_protocol`.
- [ ] Commit as `Validate realtime PCM chunk alignment`.

### Task 5: Graceful Shutdown

**Files:**
- Modify: `crates/aximo/src/config.rs`
- Modify: `crates/aximo/src/runtime.rs`
- Modify: `config/aximo.example.toml`
- Modify: `config/aximo.local.toml`
- Modify: `README.md`
- Test: `crates/aximo/src/config.rs`

- [ ] Add `shutdown_grace_period_ms` to settings and env overlays.
- [ ] Use `axum::serve(...).with_graceful_shutdown(...)` with SIGTERM/SIGINT handling.
- [ ] Document shutdown semantics for Docker/k8s.
- [ ] Run config tests.
- [ ] Commit as `Add graceful shutdown signal handling`.

### Task 6: Documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/architecture.md`
- Modify: `docs/realtime-protocol.md`

- [ ] Document that scheduler limits are admission limits, while `EngineRuntime` gates real model execution.
- [ ] Keep `segments`, `detected_language`, and linear resampling limitations explicitly documented as backend/product limitations.
- [ ] Document remaining future work: better resampler, per-request options, timestamps/language when backend exposes them, cargo audit/deny/SBOM.
- [ ] Run docs/API tests.
- [ ] Commit as `Document runtime hardening semantics`.
