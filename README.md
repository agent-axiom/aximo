# Aximo

[![Coverage](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/agent-axiom/aximo/main/badges/coverage.json)](https://github.com/agent-axiom/aximo/actions/workflows/ci.yml)

`aximo` is a CPU-first STT microservice for Russian and English built as a Rust Cargo workspace. It exposes:

- `POST /v1/transcriptions` for short audio
- `GET /v1/realtime` for realtime WebSocket streaming
- `GET /openapi.json` for the OpenAPI schema
- `GET /docs/` for Swagger UI with a built-in microphone recorder panel

## Workspace

- `crates/aximo`: HTTP and WebSocket service binary
- `crates/aximo-core`: scheduler and shared STT domain types
- `crates/aximo-inference`: `transcribe-rs` adapters for local CPU models
- `crates/aximo-audio`: audio helpers

Architecture and protocol details live in:

- [docs/architecture.md](docs/architecture.md)
- [docs/client-examples.md](docs/client-examples.md)
- [docs/realtime-protocol.md](docs/realtime-protocol.md)
- [docs/model-licenses.md](docs/model-licenses.md)

## Models

Models are runtime artifacts and must live outside git. The service expects a model root directory configured via [config/aximo.example.toml](config/aximo.example.toml).

Compatible model bundles for the current `transcribe-rs` integration:

- Parakeet int8 ONNX bundle: [blob.handy.computer/parakeet-v3-int8.tar.gz](https://blob.handy.computer/parakeet-v3-int8.tar.gz)
- Parakeet int8 ONNX bundle on Hugging Face: [istupakov/parakeet-tdt-0.6b-v3-onnx](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/tree/main)
- GigaAM v3 ONNX bundle on Hugging Face: [istupakov/gigaam-v3-onnx](https://huggingface.co/istupakov/gigaam-v3-onnx/tree/main)

Example layout:

```text
/var/lib/aximo/models/
├── parakeet-tdt-0.6b-v3-int8/
└── giga-am-v3/
```

## Quick Start

Download the default `Parakeet` model bundle:

```bash
just setup-models
```

or directly:

```bash
./scripts/fetch-models.sh
```

### Docker Compose

After the model is downloaded to `./var/models`:

```bash
docker compose up --build
```

This uses [docker-compose.yml](docker-compose.yml), mounts `./var/models` into the container, and serves the API on `http://127.0.0.1:8080`.

## Local Run

For local non-Docker usage, use [config/aximo.local.toml](config/aximo.local.toml), which points to `./var/models`:

```bash
AXIMO_CONFIG=config/aximo.local.toml cargo run -p aximo
```

For containerized usage, [config/aximo.example.toml](config/aximo.example.toml) remains the default and expects models at `/var/lib/aximo/models`.

## Configuration

`AXIMO_CONFIG` points to a TOML config file. If it is not set, built-in defaults are used. Individual fields can be overridden with environment variables after the TOML file is loaded, which is useful for Docker and Kubernetes deployments:

```bash
AXIMO_SERVER_HOST=0.0.0.0
AXIMO_SERVER_PORT=8080
AXIMO_MODELS_DIR=/var/lib/aximo/models
AXIMO_DEFAULT_OFFLINE_ENGINE=parakeet
AXIMO_DEFAULT_REALTIME_ENGINE=parakeet
AXIMO_MAX_SHORT_AUDIO_REQUESTS=8
AXIMO_MAX_SHORT_AUDIO_BYTES=25000000
AXIMO_MAX_SHORT_RAW_PCM_BYTES=1920000
AXIMO_MAX_SHORT_AUDIO_DURATION_MS=60000
AXIMO_MAX_SHORT_DECODED_SAMPLES=5760000
AXIMO_MAX_REALTIME_SESSIONS=24
AXIMO_MAX_SHORT_INFERENCES=1
AXIMO_MAX_REALTIME_INFERENCES=1
AXIMO_MAX_REALTIME_SESSION_BYTES=1920000
AXIMO_MAX_REALTIME_SESSION_DURATION_MS=60000
AXIMO_REALTIME_PARTIAL_MIN_INTERVAL_MS=300
AXIMO_REALTIME_PARTIAL_MIN_CHUNK_BYTES=9600
AXIMO_REALTIME_EVENT_CHANNEL_CAPACITY=64
```

## Short Audio Example

Short transcription currently accepts:

- `audio/wav`
- `audio/mpeg`
- `audio/flac`
- `audio/mp4`
- `audio/x-m4a`
- `audio/pcm`
- `application/octet-stream`

Compressed/container formats are decoded and normalized before inference. `audio/pcm` and `application/octet-stream` are still interpreted as raw `pcm_s16le`, `16 kHz`, mono audio. Short-audio ingest is bounded by HTTP body size, raw PCM byte size, decoded sample count, and decoded duration; limit violations return `413 Payload Too Large`.

```bash
curl -X POST http://127.0.0.1:8080/v1/transcriptions \
  -H 'content-type: audio/wav' \
  --data-binary @sample.wav
```

Example response:

```json
{
  "text": "hello world",
  "segments": [],
  "detected_language": null,
  "engine": "parakeet",
  "duration_ms": 1000,
  "processing_ms": 37
}
```

With the current `transcribe-rs` ONNX adapters used here, `detected_language` is `null` when language detection is not exposed and `segments` stays empty when segmentation or timestamps are unavailable. `duration_ms` and `processing_ms` are measured values and vary per request.

Error responses from `POST /v1/transcriptions` are structured JSON:

```json
{
  "code": "invalid_audio",
  "message": "invalid audio payload: pcm payload must be aligned to 16-bit samples"
}
```

Unsupported short-audio media types return `415 Unsupported Media Type` with code `unsupported_media_type`. Malformed payloads for supported media types remain `400 invalid_audio`.

## Realtime Example

Realtime uses WebSocket and raw `pcm_s16le`, `16 kHz`, mono binary chunks. This is bounded buffered realtime, not a true incremental streaming decoder.
Partial hypotheses are computed from a bounded rolling recent window and use latest-wins coalescing under load, so they favor freshness over a steady partial cadence. The final transcription on `stop` waits for the realtime inference slot and runs over the full bounded session buffer.
Admission limits and inference limits are configured separately: `max_short_audio_requests` and `max_realtime_sessions` bound accepted work, while `max_short_inferences` and `max_realtime_inferences` bound actual concurrent model executions per engine instance.
Realtime server events are sent through a bounded per-socket queue; clients that stop reading can be disconnected instead of accumulating unbounded memory.

```js
const ws = new WebSocket("ws://127.0.0.1:8080/v1/realtime");
ws.binaryType = "arraybuffer";

ws.addEventListener("message", (event) => {
  console.log("server:", event.data);
});

ws.addEventListener("open", async () => {
  ws.send(JSON.stringify({ event: "start" }));

  const pcmChunk = new Uint8Array([0, 0, 1, 0, 2, 0, 3, 0]);
  ws.send(pcmChunk);

  ws.send(JSON.stringify({ event: "stop" }));
});
```

Expected server events:

- `session_started`
- `partial`
- `final`
- `error`

`error` events now include machine-readable `code` and human-readable `reason`, for example:

```json
{
  "event": "error",
  "code": "realtime_capacity_exhausted",
  "reason": "realtime session capacity exhausted"
}
```

## API Docs

After the service starts:

- Swagger UI: [http://127.0.0.1:8080/docs/](http://127.0.0.1:8080/docs/)
- OpenAPI JSON: [http://127.0.0.1:8080/openapi.json](http://127.0.0.1:8080/openapi.json)
- Client examples: [docs/client-examples.md](docs/client-examples.md)

The `/docs/` page also includes an `Aximo Recorder` panel that can capture microphone audio in the browser:

- `Short Audio` records locally, converts to WAV, and sends the result to `POST /v1/transcriptions`
- `Realtime` downsamples to `pcm_s16le 16 kHz mono` and streams binary chunks to `GET /v1/realtime`

For browser microphone access, use `localhost`, `127.0.0.1`, or HTTPS.

One notable addition: I extended Swagger to support sending recordings directly from the microphone.

![Aximo Swagger recorder](docs/assets/swagger-recorder.png)

## Troubleshooting

If container logs include `onnxruntime cpuid_info warning: Unknown CPU vendor`, this is typically an ONNX Runtime CPU detection warning on ARM or virtualized environments, not a model-load failure. The container now sets `ORT_LOG=error` to reduce that noise in normal runs.

## Development

Common checks:

```bash
just fmt
just lint
just test
just coverage
just setup-models
```

## crates.io

The publishable library crates are:

- `aximo-core`
- `aximo-audio`
- `aximo-inference`

The `aximo` service crate is intentionally marked `publish = false`.

Use `just package-libs` for the local pre-publish check of `aximo-core` and `aximo-audio`. `aximo-inference` must be dry-run published only after `aximo-core` is already available in the `crates.io` index.

Release workflow notes are documented in [docs/publishing.md](docs/publishing.md).
