# Installation and first run

Aximo is a service binary built from a Rust workspace. It is not a microphone CLI: clients send audio to its HTTP/WebSocket API. The `aximo` binary is not published on crates.io, so `cargo install aximo` is not the installation path.

## Prerequisites

For all paths:

- Git, Bash, curl, and tar for the checkout and model downloader
- Internet access for the initial source, dependency, runtime, and model downloads
- Disk and RAM for the model plus build artifacts; model loading and inference are CPU-intensive

For a native build:

- [Rust installed through rustup](https://www.rust-lang.org/tools/install); the checkout pins the tested toolchain in `rust-toolchain.toml`
- A native C/C++ build toolchain and `pkg-config`; on Debian/Ubuntu, install `build-essential pkg-config libssl-dev`
- On macOS, install the Xcode Command Line Tools; use the repository-pinned Rust toolchain

The ONNX Runtime dependency downloads platform-specific binaries during the build. The supplied Dockerfile uses Debian trixie because some ARM64 runtime binaries require newer glibc/libstdc++ than bookworm. The release image targets Linux AMD64; other native platforms must be verified on the target machine.

For containers, install [Docker with Compose](https://docs.docker.com/compose/install/). A host Rust installation is not required.

## Get the source and model

```bash
git clone https://github.com/agent-axiom/aximo.git
cd aximo
./scripts/fetch-models.sh
```

The script downloads the default Parakeet v3 int8 ONNX bundle into `var/models/parakeet-tdt-0.6b-v3-int8/`. Models are runtime data and are not committed to Git. `just setup-models` is an optional alias if you have `just` installed.

Compatible model sources:

- [Parakeet v3 int8 archive](https://blob.handy.computer/parakeet-v3-int8.tar.gz), used by the script
- [Parakeet ONNX files](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/tree/main)
- [GigaAM v3 ONNX files](https://huggingface.co/istupakov/gigaam-v3-onnx/tree/main)

Check [model licenses](model-licenses.md) before redistribution. GigaAM is optional; it is not downloaded by the default script. To select it, supply its model directory and set both `AXIMO_DEFAULT_OFFLINE_ENGINE=gigaam` and `AXIMO_DEFAULT_REALTIME_ENGINE=gigaam`. The request's `engine` query parameter does not hot-swap models.

## Run with Docker Compose

From the checkout after downloading the model:

```bash
docker compose up --build
```

Compose mounts `var/models` read-only, loads `config/aximo.example.toml`, and publishes port 8080 on the host's loopback interface. It builds the image from this checkout. To serve other machines, deliberately change the port binding and follow [deployment security](deployment-security.md).

## Build and run natively

```bash
cargo build --release --locked -p aximo
AXIMO_CONFIG=config/aximo.local.toml ./target/release/aximo
```

`config/aximo.local.toml` binds `127.0.0.1:8080` and uses `./var/models`. Run it from the repository root so relative paths resolve correctly. The service loads the selected models before it starts accepting requests.

For development, the equivalent command is:

```bash
AXIMO_CONFIG=config/aximo.local.toml cargo run --locked -p aximo
```

## Install the binary from the checkout

```bash
cargo install --locked --path crates/aximo
AXIMO_CONFIG="$PWD/config/aximo.local.toml" \
  AXIMO_MODELS_DIR="$PWD/var/models" aximo
```

`cargo install` installs the binary under Cargo's bin directory, normally `~/.cargo/bin`. It does not install models or configuration. Use absolute paths, as above, when running outside the checkout. Ensure Cargo's bin directory is on your `PATH`.

There is no dedicated `--help`/`--version` command yet: the current binary starts the server, so use the API checks below to verify it.

## Verify the running service

In another terminal:

```bash
curl --fail-with-body http://127.0.0.1:8080/health/ready
curl --fail-with-body http://127.0.0.1:8080/v1/capabilities
```

Open [the recorder and API docs](http://127.0.0.1:8080/docs/). Browser microphone access requires localhost/loopback or HTTPS and your explicit permission.

Send an existing WAV file:

```bash
curl --fail-with-body --max-time 130 \
  http://127.0.0.1:8080/v1/transcriptions \
  -H 'Content-Type: audio/wav' --data-binary @sample.wav
```

The response contains recognized `text`. Audio is the raw request body, not a multipart form. Default short-audio limits include a 60-second duration cap; see [configuration](configuration.md). Run the repeatable API smoke check with Python 3:

```bash
python3 scripts/smoke-test.py
```

The smoke check uses generated silence, checks service endpoints and real inference, and does not access a microphone. It verifies service wiring, not recognition quality. For speech accuracy, test representative Russian/English recordings on the selected model and hardware.

## Troubleshooting

- **Model missing or invalid:** check all required model files and the working directory. To retry a damaged download, run `AXIMO_FORCE=1 ./scripts/fetch-models.sh`.
- **Linker/OpenSSL build errors:** install the native build prerequisites above. Linux containers provide a reproducible alternative.
- **Port already used:** choose another `AXIMO_SERVER_PORT` and adjust your client URL.
- **HTTP 413:** the recording exceeds configured audio limits. Shorten it or intentionally raise the relevant limits.
- **HTTP 429 or 504:** the instance is saturated or inference exceeded its timeout. Stop competing requests and see [operations](operations.md).

Further reading: [API reference](api-reference.md), [configuration](configuration.md), [terminal voice input](terminal-voice.md), and [development](development.md).
