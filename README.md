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

## Short Audio Example

Short transcription currently accepts:

- `audio/wav`
- `audio/pcm`
- `application/octet-stream`

`audio/pcm` and `application/octet-stream` are interpreted as raw `pcm_s16le`, `16 kHz`, mono audio.

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

`detected_language` is `null` when the current engine integration does not expose language detection. `segments` stays empty when segmentation or timestamps are unavailable. `duration_ms` and `processing_ms` are measured values and vary per request.

## Realtime Example

Realtime uses WebSocket and raw `pcm_s16le`, `16 kHz`, mono binary chunks.
Partial hypotheses are computed from a bounded rolling recent window, while the final transcription on `stop` uses the full buffered session audio.
Admission limits and inference limits are configured separately: `max_short_audio_requests` and `max_realtime_sessions` bound accepted work, while `max_short_inferences` and `max_realtime_inferences` bound actual concurrent model executions per engine instance.

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
