# Aximo Architecture

`aximo` is a CPU-first STT microservice for Russian and English built as a Cargo workspace.

## Components

```mermaid
flowchart LR
    Client["HTTP / WebSocket client"] --> API["crates/aximo"]
    API --> Core["crates/aximo-core"]
    API --> Inference["crates/aximo-inference"]
    API --> Audio["crates/aximo-audio"]
    API --> Metrics["/metrics"]
    Inference --> Models["Local model directory"]
```

## Request Flow

### Short Audio

```mermaid
sequenceDiagram
    participant C as Client
    participant A as aximo
    participant S as Scheduler
    participant E as Offline Engine

    C->>A: POST /v1/transcriptions
    A->>S: acquire short request permit
    S-->>A: request permit
    A->>S: acquire short inference permit
    S-->>A: inference permit
    A->>E: transcribe_short()
    E-->>A: transcript
    A-->>C: JSON response
```

### Realtime

Realtime is intentionally implemented as bounded buffered realtime. The service accepts live WebSocket chunks and emits partial/final events, but the current `transcribe-rs` path still runs bounded offline decodes rather than a true incremental streaming decoder.

```mermaid
sequenceDiagram
    participant C as Client
    participant W as WebSocket handler
    participant M as SessionManager
    participant S as Scheduler
    participant P as Partial Worker
    participant E as Realtime Engine

    C->>W: start
    W->>S: acquire realtime session permit
    S-->>W: session permit
    W->>M: create session
    W-->>C: session_started
    C->>W: binary audio chunk
    W->>M: append chunk
    W->>M: evaluate partial cadence / inflight state
    W->>P: spawn partial job when eligible
    P->>S: await realtime inference permit
    S-->>P: inference permit
    P->>E: transcribe_short(latest rolling buffer)
    E-->>P: partial text
    P-->>C: partial
    Note over W,P: stale partial demand collapses into one latest follow-up partial
    C->>W: stop
    W->>M: finish session
    W->>S: await realtime inference permit
    S-->>W: inference permit
    W->>E: transcribe_short(final buffer)
    E-->>W: final text
    W-->>C: final
```

## Runtime Model Convention

- Models live outside git.
- `Settings.inference.models_dir` points to the root directory.
- `default_offline_engine` and `default_realtime_engine` choose named engines from config.
- The current implementation supports `parakeet` and `gigaam` through `transcribe-rs`.
- `max_short_audio_requests` and `max_realtime_sessions` bound admitted work.
- `max_short_inferences` and `max_realtime_inferences` bound per-path inference admission and should reflect how much work the service should queue toward each path.
- When the same offline and realtime engine config resolves to the same backend/model path, Aximo reuses one engine instance to avoid loading duplicate model copies. That saves RAM. Actual backend calls are additionally protected by a per-engine execution gate that is shared by offline and realtime when they share an engine `Arc`; the gate remains held until a blocking backend call exits, even if the client already received a timeout.
- Realtime partials are best-effort and latest-wins under saturation; final transcriptions remain strict and run against the full bounded session buffer.
- `segments` and `detected_language` are capability-dependent response fields. The current `transcribe-rs` ONNX adapter path exposes plain transcript text, measured duration, and measured processing time, but not segment timestamps or real language detection.

## Observability

`GET /metrics` returns Prometheus-compatible text metrics for request status/code counts, error codes, audio body size, decoded audio duration, decode time, scheduler wait, model execution wait, inference wall time, realtime factor, inference timeouts, active blocking tasks, active model executions, active websocket sessions, queue overflows, stale partial skips, and coalesced realtime partials.

`GET /health/live` is process liveness. `GET /health/ready` reflects runtime health and returns `503` after consecutive timeout/runtime/unavailable inference failures cross the configured degradation threshold.
