# API Operational Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tighten external API semantics and deployment ergonomics without pretending the current engine is a true streaming decoder.

**Architecture:** Keep the existing Rust workspace boundaries. HTTP error mapping stays in `aximo`, audio capability classification stays in `aximo-audio`, and settings remain TOML-first with an optional environment overlay applied after file/default loading.

**Tech Stack:** Rust, axum, serde, toml, rstest-style integration tests already present in the workspace.

---

### Task 1: Return 415 For Unsupported Short-Audio Media Types

**Files:**
- Modify: `crates/aximo-audio/src/decode.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Modify: `crates/aximo/tests/transcriptions_api.rs`
- Modify: `crates/aximo/src/docs.rs`
- Modify: `README.md`

- [ ] Add a regression test that posts an unsupported `Content-Type` and expects `415 Unsupported Media Type`.
- [ ] Map `AudioError::UnsupportedContentType` to code `unsupported_media_type`.
- [ ] Keep malformed supported audio as `400 invalid_audio`.
- [ ] Update OpenAPI and README error examples.
- [ ] Run targeted transcription API tests and commit.

### Task 2: Add Per-Field Environment Overlay

**Files:**
- Modify: `crates/aximo/src/config.rs`
- Modify: `crates/aximo/tests/runtime_config.rs`
- Modify: `README.md`
- Modify: `config/aximo.example.toml`

- [ ] Add tests proving env vars override TOML/default values after `AXIMO_CONFIG` loading.
- [ ] Support env overrides for server host/port, model directory, default engines, and all operational limits.
- [ ] Parse numeric env values with contextual errors.
- [ ] Document the supported env vars for Docker/Kubernetes use.
- [ ] Run config tests and commit.

### Task 3: Make Realtime Mode Explicit In API Docs

**Files:**
- Modify: `README.md`
- Modify: `docs/realtime-protocol.md`
- Modify: `docs/architecture.md`
- Modify: `crates/aximo/src/docs.rs`

- [ ] Document realtime as bounded buffered realtime, not a true incremental streaming decoder.
- [ ] State that final transcription is computed from the full bounded session buffer.
- [ ] State that partial events use latest-wins coalescing and may not form a steady cadence.
- [ ] Keep the response metadata language honest: `segments` and `detected_language` are capability-dependent.
- [ ] Run docs/API tests and commit.

### Final Verification

- [ ] Run `cargo fmt --all --check`.
- [ ] Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- [ ] Run `cargo test --workspace --locked`.
- [ ] Run `cargo llvm-cov --workspace --summary-only --fail-under-lines 88`.
- [ ] Fast-forward merge into `main` and push after all checks pass.
