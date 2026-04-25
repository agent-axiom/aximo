# Benchmarks

Aximo includes a lightweight API benchmark harness for real service runs. It is intentionally external to `cargo test`: meaningful STT benchmarking requires a running service, mounted model files, and CPU/RAM limits that match the target deployment.

## Run

Start the service first:

```bash
AXIMO_CONFIG=config/aximo.local.toml cargo run -p aximo
```

Then run the benchmark client from another shell:

```bash
just benchmark-api
```

By default the script benchmarks the configured Parakeet service with generated 5s, 30s, and 60s WAV tones. Results are written to `target/aximo-benchmarks/results.csv` and summarized in `target/aximo-benchmarks/summary.txt`.

The generated tone path is only a mechanics smoke test for decode overhead, latency, and RTF. To measure recognition quality, point the harness at real speech fixtures and provide `.txt` transcripts with matching basenames:

```text
bench-fixtures/
  ru-clean-5s.wav
  ru-clean-5s.txt
  en-noisy-30s.mp3
  en-noisy-30s.txt
```

Then run:

```bash
AXIMO_BENCH_FIXTURES_DIR=bench-fixtures AXIMO_BENCH_FORMATS="wav mp3 m4a flac" just benchmark-api
```

When transcript sidecars are present, `results.csv` and `summary.txt` include WER and CER columns. Use real RU/EN, clean/noisy, short/medium/long samples for production evidence; synthetic tones should not be interpreted as STT quality data.

## Options

```bash
AXIMO_BENCH_BASE_URL=http://127.0.0.1:8080
AXIMO_BENCH_ENGINES=parakeet
AXIMO_BENCH_FORMATS="wav mp3 m4a"
AXIMO_BENCH_DURATIONS="5 30 60"
AXIMO_BENCH_RUNS=10
AXIMO_BENCH_WARMUPS=2
AXIMO_BENCH_LANGUAGE=ru
AXIMO_BENCH_TIMESTAMPS=true
AXIMO_BENCH_FIXTURES_DIR=bench-fixtures
AXIMO_BENCH_EXPECTED_DIR=bench-transcripts
just benchmark-api
```

`mp3`, `m4a`, and `flac` benchmarks require `ffmpeg`; the default `wav` path only uses Python standard-library audio generation.

## Parakeet And GigaAM

The HTTP endpoint validates `engine` against the service instance's configured offline engine. To compare Parakeet and GigaAM, run the service once with `default_offline_engine = "parakeet"` and once with `default_offline_engine = "gigaam"`, or run two service instances on different ports:

```bash
AXIMO_BENCH_BASE_URL=http://127.0.0.1:8080 AXIMO_BENCH_ENGINES=parakeet just benchmark-api
AXIMO_BENCH_BASE_URL=http://127.0.0.1:8081 AXIMO_BENCH_ENGINES=gigaam just benchmark-api
```

Track at least:

- latency p50/p95/p99 from the client side;
- `processing_ms` reported by the API;
- RTF (`processing_ms / duration_ms`);
- peak RSS and CPU from the runtime environment;
- `/metrics` series such as `aximo_inference_seconds_bucket`, `aximo_model_execution_wait_seconds_bucket`, and `aximo_rtf_bucket`.

## Interpretation

For CPU-only STT, RTF below `1.0` means inference is faster than realtime for the measured audio duration. Run cold and warm passes separately: cold starts include model load and allocator effects, while warm passes show steady-state request performance.
