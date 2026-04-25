# Benchmark Baselines

This page records reproducible benchmark evidence for Aximo API mechanics. It is not a replacement for production WER/CER validation on a curated human-speech dataset.

## Local Parakeet RU/EN TTS Smoke

Generated on 2026-04-26 from local macOS `say` fixtures produced by `scripts/generate-benchmark-fixtures.sh`, then transcribed through a locally running Aximo service with `config/aximo.local.toml` and the `parakeet-tdt-0.6b-v3-int8` ONNX bundle.

Reproduction:

```bash
AXIMO_CONFIG=config/aximo.local.toml RUST_LOG=error cargo run -p aximo
AXIMO_BENCH_FIXTURE_FORMATS=wav \
AXIMO_BENCH_FIXTURE_OUT_DIR=target/aximo-benchmarks/fixtures \
just benchmark-fixtures
AXIMO_BENCH_FIXTURES_DIR=target/aximo-benchmarks/fixtures \
AXIMO_BENCH_RUNS=1 \
AXIMO_BENCH_WARMUPS=0 \
AXIMO_BENCH_ENGINES=parakeet \
AXIMO_BENCH_OUT_DIR=target/aximo-benchmarks/local-parakeet-tts \
AXIMO_BENCH_FIXTURE_SET="macOS say RU/EN TTS smoke fixtures" \
AXIMO_BENCH_MODEL_SET="local parakeet-tdt-0.6b-v3-int8" \
just benchmark-api
```

Results:

| engine | format | duration_s | sample | runs | latency_p50_ms | latency_p95_ms | latency_p99_ms | rtf_avg | wer_avg | cer_avg |
| --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| parakeet | wav | 4.141 | en-tts-5s | 1 | 232.437 | 232.437 | 232.437 | 0.055542 | 0.083333 | 0.028169 |
| parakeet | wav | 4.304 | ru-tts-5s | 1 | 215.458 | 215.458 | 215.458 | 0.049489 | 0.111111 | 0.027027 |

Interpretation:

- This is a local smoke benchmark proving the API, decode path, model execution, report pipeline, and WER/CER sidecar flow work end-to-end.
- The fixtures are synthetic speech, so these WER/CER values must not be presented as production recognition quality.
- Use real RU/EN human speech fixtures for production evidence, ideally across clean/noisy, short/medium/long, and different speakers.
