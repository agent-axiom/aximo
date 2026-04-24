# Production Hardening Limits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bound the main remaining memory/CPU pressure points before widening the service further.

**Architecture:** Keep the service Rust-only and preserve existing crate ownership. `aximo` owns HTTP and WebSocket admission behavior, `aximo-audio` owns decode/normalization limits, `aximo-core` owns realtime session state, and runtime engine loading stays in `aximo`.

**Tech Stack:** Rust, axum, tokio, hound, symphonia, existing test stack.

---

### Task 1: Bound Short-Audio HTTP And Decode Work

**Files:**
- Modify: `crates/aximo/src/config.rs`
- Modify: `crates/aximo/src/app.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo-audio/src/decode.rs`
- Modify: `crates/aximo-audio/src/normalize.rs`
- Modify: `crates/aximo-audio/src/lib.rs`
- Modify: `crates/aximo/tests/transcriptions_api.rs`
- Modify: `config/aximo.example.toml`
- Modify: `config/aximo.local.toml`
- Modify: `README.md`
- Modify: `crates/aximo/src/docs.rs`

- [ ] Add failing tests for `413 Payload Too Large` on HTTP body, raw PCM bytes, decoded samples, and decoded duration.
- [ ] Add `max_short_audio_bytes`, `max_short_raw_pcm_bytes`, `max_short_audio_duration_ms`, and `max_short_decoded_samples`.
- [ ] Apply explicit `DefaultBodyLimit` on `/v1/transcriptions`.
- [ ] Enforce decode and duration limits before inference.
- [ ] Map limit violations to structured `413 payload_too_large`.
- [ ] Document defaults and env overrides.
- [ ] Commit as `Bound short-audio ingest work`.

### Task 2: Deduplicate Default Engine Loading

**Files:**
- Modify: `crates/aximo/src/runtime.rs`
- Modify: `crates/aximo/tests/runtime_config.rs`

- [ ] Add a failing test for equal offline/realtime `EngineSpec` reuse.
- [ ] Add `load_default_engines` that loads one engine and clones the `Arc` when specs are equal.
- [ ] Keep separate loads when engine specs differ.
- [ ] Update `run_service` to use the deduplicating loader.
- [ ] Commit as `Deduplicate matching default engine loads`.

### Task 3: Avoid Stale Partial Inference After Session Close

**Files:**
- Modify: `crates/aximo-core/src/realtime.rs`
- Modify: `crates/aximo-core/tests/realtime.rs`
- Modify: `crates/aximo/src/ws/handler.rs`
- Modify: `crates/aximo/tests/realtime_protocol.rs`

- [ ] Add failing tests proving a partial task waiting for an inference slot does not run inference after its session is stopped.
- [ ] Reorder partial work so it acquires the inference permit before taking the audio snapshot.
- [ ] Re-check session liveness after permit acquisition and before calling inference.
- [ ] Commit as `Skip stale realtime partial inference`.

### Task 4: Bound WebSocket Event Queue

**Files:**
- Modify: `crates/aximo/src/config.rs`
- Modify: `crates/aximo/src/app.rs`
- Modify: `crates/aximo/src/ws/handler.rs`
- Modify: `crates/aximo/tests/runtime_config.rs`
- Modify: `docs/realtime-protocol.md`
- Modify: `README.md`

- [ ] Add `realtime_event_channel_capacity` with a conservative default.
- [ ] Replace the unbounded event channel with a bounded channel.
- [ ] Treat event queue overflow as connection termination.
- [ ] Document that slow readers can be disconnected.
- [ ] Commit as `Bound realtime event queue`.

### Deferred Follow-Ups

- Metrics: add a proper metrics exporter with queue wait, inference wait, decode time, duration, RTF, and errors by code.
- Resampler: replace the current linear resampler with a higher-quality implementation after choosing the dependency.
- Richer metadata: fill `segments` and `detected_language` only when the active STT backend exposes real values.

### Final Verification

- [ ] Run `cargo fmt --all --check`.
- [ ] Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- [ ] Run `cargo test --workspace --locked`.
- [ ] Run `cargo llvm-cov --workspace --summary-only --fail-under-lines 88`.
- [ ] Fast-forward merge into `main` and push after all checks pass.
