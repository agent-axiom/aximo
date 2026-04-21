# Aximo Architecture

`aximo` is a CPU-first STT microservice for Russian and English built as a Cargo workspace.

## Components

```mermaid
flowchart LR
    Client["HTTP / WebSocket client"] --> API["crates/aximo"]
    API --> Core["crates/aximo-core"]
    API --> Inference["crates/aximo-inference"]
    API --> Audio["crates/aximo-audio"]
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
    A->>S: acquire short-audio permit
    S-->>A: permit
    A->>E: transcribe_short()
    E-->>A: transcript
    A-->>C: JSON response
```

### Realtime

```mermaid
sequenceDiagram
    participant C as Client
    participant W as WebSocket handler
    participant M as SessionManager
    participant S as Scheduler
    participant E as Realtime Engine

    C->>W: start
    W->>S: acquire realtime permit
    S-->>W: permit
    W->>M: create session
    W-->>C: session_started
    C->>W: binary audio chunk
    W->>M: append chunk
    W->>E: transcribe_short(rolling buffer)
    E-->>W: partial text
    W-->>C: partial
    C->>W: stop
    W->>M: finish session
    W->>E: transcribe_short(final buffer)
    E-->>W: final text
    W-->>C: final
```

## Runtime Model Convention

- Models live outside git.
- `Settings.inference.models_dir` points to the root directory.
- `default_offline_engine` and `default_realtime_engine` choose named engines from config.
- The current implementation supports `parakeet` and `gigaam` through `transcribe-rs`.
