# Aximo STT Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a CPU-first Rust STT microservice with a short-audio HTTP API and a realtime WebSocket API using local models through `transcribe-rs`.

**Architecture:** A Cargo workspace split into API, core, inference, and audio crates. The service uses bounded worker pools and admission control to protect realtime traffic while keeping model files outside git and outside crate source paths.

**Tech Stack:** Rust, axum, tokio, serde, tracing, utoipa, transcribe-rs, rstest, insta, proptest, cargo-nextest, cargo-llvm-cov, just

---

### Task 1: Create Workspace Skeleton and Tooling

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.cargo/config.toml`
- Create: `justfile`
- Create: `config/aximo.example.toml`
- Create: `crates/aximo/Cargo.toml`
- Create: `crates/aximo-core/Cargo.toml`
- Create: `crates/aximo-audio/Cargo.toml`
- Create: `crates/aximo-inference/Cargo.toml`
- Test: `tests/workspace_smoke.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn workspace_exposes_expected_crates() {
    let manifest = std::fs::read_to_string("Cargo.toml").unwrap();
    assert!(manifest.contains("crates/aximo"));
    assert!(manifest.contains("crates/aximo-core"));
    assert!(manifest.contains("crates/aximo-audio"));
    assert!(manifest.contains("crates/aximo-inference"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test workspace_smoke workspace_exposes_expected_crates`
Expected: FAIL because the workspace manifest and test target do not exist yet

- [ ] **Step 3: Write minimal implementation**

```toml
[workspace]
members = [
  "crates/aximo",
  "crates/aximo-core",
  "crates/aximo-audio",
  "crates/aximo-inference",
]
resolver = "2"
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test workspace_smoke workspace_exposes_expected_crates`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .cargo/config.toml justfile config/aximo.example.toml crates tests
git commit -m "chore: scaffold aximo workspace"
```

### Task 2: Add Configuration and Health Endpoints

**Files:**
- Create: `crates/aximo/src/main.rs`
- Create: `crates/aximo/src/app.rs`
- Create: `crates/aximo/src/config.rs`
- Create: `crates/aximo/src/http/health.rs`
- Test: `tests/health_api.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn readiness_endpoint_returns_ok() {
    let app = aximo::app::build_test_app().await;
    let response = axum_test::TestServer::new(app).unwrap().get("/health/ready").await;
    response.assert_status_ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test health_api readiness_endpoint_returns_ok`
Expected: FAIL because the app builder and route do not exist

- [ ] **Step 3: Write minimal implementation**

```rust
pub async fn ready() -> impl IntoResponse {
    StatusCode::OK
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test health_api readiness_endpoint_returns_ok`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/aximo tests/health_api.rs
git commit -m "feat: add config and health endpoints"
```

### Task 3: Implement Short-Audio Use Case with Fake Inference

**Files:**
- Create: `crates/aximo-core/src/short_audio.rs`
- Create: `crates/aximo-inference/src/engine.rs`
- Create: `crates/aximo/src/http/transcriptions.rs`
- Test: `tests/transcriptions_api.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn transcription_endpoint_returns_fake_engine_result() {
    let app = aximo::app::build_test_app().await;
    let response = axum_test::TestServer::new(app)
        .unwrap()
        .post("/v1/transcriptions")
        .bytes(vec![0_u8; 3200], "audio/wav")
        .await;

    response.assert_status_ok();
    response.assert_json_contains(serde_json::json!({
        "text": "hello world",
        "engine": "fake"
    }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test transcriptions_api transcription_endpoint_returns_fake_engine_result`
Expected: FAIL because the route and fake engine path do not exist

- [ ] **Step 3: Write minimal implementation**

```rust
pub struct FakeEngine;

impl SpeechEngine for FakeEngine {
    fn transcribe_short(&self, _request: ShortAudioRequest) -> ShortAudioResult {
        ShortAudioResult::new("hello world", "fake")
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test transcriptions_api transcription_endpoint_returns_fake_engine_result`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates tests/transcriptions_api.rs
git commit -m "feat: add short audio transcription api"
```

### Task 4: Implement Realtime Session Lifecycle

**Files:**
- Create: `crates/aximo-core/src/realtime.rs`
- Create: `crates/aximo/src/ws/protocol.rs`
- Create: `crates/aximo/src/ws/handler.rs`
- Test: `tests/realtime_protocol.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn websocket_session_emits_started_and_final_events() {
    let (mut client, _server) = connect_test_ws().await;
    client.send_start().await;
    client.send_stop().await;

    assert_eq!(client.next_event_type().await, "session_started");
    assert_eq!(client.next_event_type().await, "final");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test realtime_protocol websocket_session_emits_started_and_final_events`
Expected: FAIL because the WebSocket handler and protocol do not exist

- [ ] **Step 3: Write minimal implementation**

```rust
match message.event.as_str() {
    "start" => send(session_started()),
    "stop" => send(final_event("hello world")),
    _ => send(error_event("unsupported_event")),
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test realtime_protocol websocket_session_emits_started_and_final_events`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates tests/realtime_protocol.rs
git commit -m "feat: add realtime websocket session flow"
```

### Task 5: Add Scheduler and Admission Control

**Files:**
- Create: `crates/aximo-core/src/scheduler.rs`
- Modify: `crates/aximo-core/src/realtime.rs`
- Modify: `crates/aximo/src/http/transcriptions.rs`
- Test: `tests/admission_control.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn scheduler_rejects_when_realtime_capacity_is_exhausted() {
    let scheduler = Scheduler::new(1);
    let first = scheduler.try_acquire_realtime();
    let second = scheduler.try_acquire_realtime();

    assert!(first.is_ok());
    assert!(second.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test admission_control scheduler_rejects_when_realtime_capacity_is_exhausted`
Expected: FAIL because the scheduler does not exist

- [ ] **Step 3: Write minimal implementation**

```rust
pub fn try_acquire_realtime(&self) -> Result<Permit, CapacityError> {
    self.realtime_semaphore
        .clone()
        .try_acquire_owned()
        .map(Permit::Realtime)
        .map_err(|_| CapacityError::RealtimeSaturated)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test admission_control scheduler_rejects_when_realtime_capacity_is_exhausted`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates tests/admission_control.rs
git commit -m "feat: add scheduler admission control"
```

### Task 6: Add Audio Windowing and Result Reconciliation

**Files:**
- Create: `crates/aximo-audio/src/windowing.rs`
- Create: `crates/aximo-core/src/reconcile.rs`
- Test: `tests/windowing_props.rs`
- Test: `tests/reconcile_snapshots.rs`

- [ ] **Step 1: Write the failing test**

```rust
proptest! {
    #[test]
    fn generated_windows_never_exceed_source_bounds(
        len in 3200_usize..96000_usize
    ) {
        let samples = vec![0_i16; len];
        for window in windows(&samples, 16000, 8000) {
            prop_assert!(window.end <= len);
            prop_assert!(window.start < window.end);
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test windowing_props generated_windows_never_exceed_source_bounds`
Expected: FAIL because the window generator does not exist

- [ ] **Step 3: Write minimal implementation**

```rust
pub fn windows(samples: &[i16], size: usize, overlap: usize) -> Vec<Window> {
    let step = size.saturating_sub(overlap).max(1);
    let mut start = 0;
    let mut out = Vec::new();
    while start < samples.len() {
        let end = (start + size).min(samples.len());
        out.push(Window { start, end });
        if end == samples.len() {
            break;
        }
        start += step;
    }
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test windowing_props generated_windows_never_exceed_source_bounds`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates tests/windowing_props.rs tests/reconcile_snapshots.rs
git commit -m "feat: add audio windowing and transcript reconciliation"
```

### Task 7: Add OpenAPI, Docs, and Coverage Pipeline

**Files:**
- Modify: `crates/aximo/src/app.rs`
- Create: `docs/architecture.md`
- Create: `docs/realtime-protocol.md`
- Create: `docs/model-licenses.md`
- Modify: `justfile`
- Test: `tests/openapi_snapshot.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn openapi_includes_transcriptions_endpoint() {
    let spec = aximo::openapi::build_openapi();
    let json = serde_json::to_value(spec).unwrap();
    assert!(json["paths"].get("/v1/transcriptions").is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test openapi_snapshot openapi_includes_transcriptions_endpoint`
Expected: FAIL because OpenAPI generation is not wired

- [ ] **Step 3: Write minimal implementation**

```rust
#[derive(OpenApi)]
#[openapi(paths(transcribe_short, health_ready))]
pub struct ApiDoc;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test openapi_snapshot openapi_includes_transcriptions_endpoint`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates docs justfile tests/openapi_snapshot.rs
git commit -m "docs: add service docs and coverage pipeline"
```

### Task 8: Verify Coverage and Merge

**Files:**
- Modify: any files required by review feedback

- [ ] **Step 1: Run focused test suites**

Run: `cargo nextest run --workspace`
Expected: PASS

- [ ] **Step 2: Run linting**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: PASS

- [ ] **Step 3: Run coverage**

Run: `cargo llvm-cov nextest --workspace --lcov --output-path target/llvm-cov/lcov.info`
Expected: PASS with at least 88% line coverage

- [ ] **Step 4: Merge branch after verification**

```bash
git checkout main
git merge --ff-only codex/verified-feature
```

- [ ] **Step 5: Commit follow-up fixes if review requires**

```bash
git add .
git commit -m "fix: address review feedback"
```
