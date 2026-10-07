# Aximo

[![CI](https://github.com/agent-axiom/aximo/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/agent-axiom/aximo/actions/workflows/ci.yml)
[![Security](https://github.com/agent-axiom/aximo/actions/workflows/security.yml/badge.svg?branch=main)](https://github.com/agent-axiom/aximo/actions/workflows/security.yml)
[![Image Security](https://github.com/agent-axiom/aximo/actions/workflows/image-security.yml/badge.svg?branch=main)](https://github.com/agent-axiom/aximo/actions/workflows/image-security.yml)
[![Container](https://github.com/agent-axiom/aximo/actions/workflows/container.yml/badge.svg?branch=main)](https://github.com/agent-axiom/aximo/actions/workflows/container.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/agent-axiom/aximo/main/badges/coverage.json)](https://github.com/agent-axiom/aximo/actions/workflows/ci.yml)

Local, CPU-first speech-to-text API in Rust. Run your own Parakeet or GigaAM models, transcribe audio over HTTP, and receive partial/final results over WebSocket. No cloud transcription account is required.

- HTTP API for WAV, MP3, FLAC, M4A/MP4, and raw PCM
- Browser microphone recorder at `/docs/`
- Health checks, capability discovery, and Prometheus metrics
- English and Russian support depends on the selected model; check `/v1/capabilities`

The bundled adapters use bounded buffered realtime, not native incremental decoding. See [capabilities and limits](docs/api-reference.md#capabilities).

## Quick start

With [Docker Compose](https://docs.docker.com/compose/install/), Git, Bash, curl, and tar installed:

```bash
git clone https://github.com/agent-axiom/aximo.git
cd aximo
./scripts/fetch-models.sh
docker compose up --build
```

The first run downloads the Parakeet model and builds the service. Open [http://127.0.0.1:8080/docs/](http://127.0.0.1:8080/docs/) after it reports ready.

Already have Rust and a native build toolchain? From the checkout:

```bash
./scripts/fetch-models.sh
cargo build --release --locked -p aximo
AXIMO_CONFIG=config/aximo.local.toml ./target/release/aximo
```

See [installation](docs/installation.md) for prerequisites, installing the binary, model setup, and verification.

## Transcribe a file

In another terminal, send an existing audio file:

```bash
curl --fail-with-body http://127.0.0.1:8080/v1/transcriptions \
  -H 'Content-Type: audio/wav' --data-binary @sample.wav
```

The JSON response contains `text`, `segments`, `engine`, and timing metadata. For voice input in your terminal or Claude Code, see [terminal voice integration](docs/terminal-voice.md).

## Documentation

- [Installation and first-run checks](docs/installation.md)
- [API reference and browser recorder](docs/api-reference.md) · [client examples](docs/client-examples.md) · [realtime protocol](docs/realtime-protocol.md)
- [Configuration](docs/configuration.md) · [operations and troubleshooting](docs/operations.md)
- [Architecture](docs/architecture.md) · [development and CI](docs/development.md)
- [Model licenses](docs/model-licenses.md) · [benchmarks](docs/benchmarks.md) · [benchmark baselines](docs/benchmark-baselines.md)
- [Deployment security](docs/deployment-security.md) · [Kubernetes](docs/kubernetes.md) · [publishing](docs/publishing.md)

## Demo and security

Try the [public browser demo](https://ifif-aximo.hf.space/docs). It may cold-start and run slowly; only upload audio you are comfortable sending to that public service. Use a local installation for private audio.

Aximo has no built-in authentication. Keep it on loopback or behind an authenticated gateway before exposing it to other machines. See [deployment security](docs/deployment-security.md) and [security reporting](SECURITY.md).
