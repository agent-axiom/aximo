#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${AXIMO_BENCH_BASE_URL:-http://127.0.0.1:8080}"
OUT_DIR="${AXIMO_BENCH_OUT_DIR:-target/aximo-benchmarks}"
ENGINES="${AXIMO_BENCH_ENGINES:-parakeet}"
FORMATS="${AXIMO_BENCH_FORMATS:-wav}"
DURATIONS="${AXIMO_BENCH_DURATIONS:-5 30 60}"
RUNS="${AXIMO_BENCH_RUNS:-5}"
WARMUPS="${AXIMO_BENCH_WARMUPS:-1}"
LANGUAGE="${AXIMO_BENCH_LANGUAGE:-}"
TIMESTAMPS="${AXIMO_BENCH_TIMESTAMPS:-false}"

mkdir -p "$OUT_DIR/audio" "$OUT_DIR/responses"

CSV="$OUT_DIR/results.csv"
SUMMARY="$OUT_DIR/summary.txt"

cat > "$CSV" <<'CSV'
timestamp,engine,format,duration_s,run,http_status,latency_ms,processing_ms,duration_ms,rtf,text_chars,error_code
CSV

python3 - "$OUT_DIR/audio" $DURATIONS <<'PY'
import math
import pathlib
import struct
import sys
import wave

out_dir = pathlib.Path(sys.argv[1])
sample_rate = 16_000
amplitude = 0.2

for duration_s in map(int, sys.argv[2:]):
    path = out_dir / f"tone-{duration_s}s.wav"
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate)
        for i in range(duration_s * sample_rate):
            sample = int(amplitude * 32767 * math.sin(2 * math.pi * 440 * i / sample_rate))
            wav.writeframesraw(struct.pack("<h", sample))
PY

if command -v ffmpeg >/dev/null 2>&1; then
    for duration_s in $DURATIONS; do
        src="$OUT_DIR/audio/tone-${duration_s}s.wav"
        for format in $FORMATS; do
            case "$format" in
                wav)
                    ;;
                mp3)
                    ffmpeg -hide_banner -loglevel error -y -i "$src" "$OUT_DIR/audio/tone-${duration_s}s.mp3"
                    ;;
                flac)
                    ffmpeg -hide_banner -loglevel error -y -i "$src" "$OUT_DIR/audio/tone-${duration_s}s.flac"
                    ;;
                m4a)
                    ffmpeg -hide_banner -loglevel error -y -i "$src" -c:a aac "$OUT_DIR/audio/tone-${duration_s}s.m4a"
                    ;;
                *)
                    echo "unsupported benchmark format: $format" >&2
                    exit 2
                    ;;
            esac
        done
    done
else
    for format in $FORMATS; do
        if [ "$format" != "wav" ]; then
            echo "ffmpeg is required for $format benchmarks; install it or use AXIMO_BENCH_FORMATS=wav" >&2
            exit 2
        fi
    done
fi

content_type_for() {
    case "$1" in
        wav) printf "audio/wav" ;;
        mp3) printf "audio/mpeg" ;;
        flac) printf "audio/flac" ;;
        m4a) printf "audio/mp4" ;;
        *) return 1 ;;
    esac
}

query_for() {
    local engine="$1"
    local query="engine=${engine}&timestamps=${TIMESTAMPS}"
    if [ -n "$LANGUAGE" ]; then
        query="${query}&language=${LANGUAGE}"
    fi
    printf "%s" "$query"
}

run_once() {
    local engine="$1"
    local format="$2"
    local duration_s="$3"
    local run_id="$4"
    local input="$OUT_DIR/audio/tone-${duration_s}s.${format}"
    local response="$OUT_DIR/responses/${engine}-${format}-${duration_s}s-${run_id}.json"
    local content_type
    content_type="$(content_type_for "$format")"

    local curl_out
    curl_out="$(
        curl \
            --silent \
            --show-error \
            --output "$response" \
            --write-out "%{http_code} %{time_total}" \
            -X POST "${BASE_URL}/v1/transcriptions?$(query_for "$engine")" \
            -H "content-type: ${content_type}" \
            --data-binary "@${input}"
    )"

    python3 - "$CSV" "$response" "$curl_out" "$engine" "$format" "$duration_s" "$run_id" <<'PY'
import csv
import datetime as dt
import json
import pathlib
import sys

csv_path = pathlib.Path(sys.argv[1])
response_path = pathlib.Path(sys.argv[2])
http_status, latency_seconds = sys.argv[3].split()
engine, fmt, duration_s, run_id = sys.argv[4:8]

try:
    payload = json.loads(response_path.read_text())
except json.JSONDecodeError:
    payload = {}

processing_ms = payload.get("processing_ms", "")
duration_ms = payload.get("duration_ms", "")
text = payload.get("text", "")
error_code = payload.get("code", "")
rtf = ""
if isinstance(processing_ms, int) and isinstance(duration_ms, int) and duration_ms > 0:
    rtf = f"{processing_ms / duration_ms:.6f}"

row = {
    "timestamp": dt.datetime.now(dt.timezone.utc).isoformat(),
    "engine": engine,
    "format": fmt,
    "duration_s": duration_s,
    "run": run_id,
    "http_status": http_status,
    "latency_ms": f"{float(latency_seconds) * 1000:.3f}",
    "processing_ms": processing_ms,
    "duration_ms": duration_ms,
    "rtf": rtf,
    "text_chars": len(text) if isinstance(text, str) else "",
    "error_code": error_code,
}

with csv_path.open("a", newline="") as fh:
    writer = csv.DictWriter(fh, fieldnames=row.keys())
    writer.writerow(row)
PY
}

echo "Benchmarking ${BASE_URL}; results: ${CSV}"

for engine in $ENGINES; do
    for format in $FORMATS; do
        for duration_s in $DURATIONS; do
            for warmup in $(seq 1 "$WARMUPS"); do
                run_once "$engine" "$format" "$duration_s" "warmup-${warmup}" >/dev/null
            done
            for run in $(seq 1 "$RUNS"); do
                run_once "$engine" "$format" "$duration_s" "$run"
            done
        done
    done
done

python3 - "$CSV" "$SUMMARY" <<'PY'
import csv
import pathlib
import statistics
import sys
from collections import defaultdict

csv_path = pathlib.Path(sys.argv[1])
summary_path = pathlib.Path(sys.argv[2])

groups = defaultdict(list)
with csv_path.open() as fh:
    for row in csv.DictReader(fh):
        if row["run"].startswith("warmup"):
            continue
        if row["http_status"] != "200":
            continue
        key = (row["engine"], row["format"], row["duration_s"])
        groups[key].append(row)

def percentile(values, pct):
    ordered = sorted(values)
    if not ordered:
        return 0.0
    index = min(len(ordered) - 1, round((pct / 100) * (len(ordered) - 1)))
    return ordered[index]

lines = []
lines.append("engine,format,duration_s,runs,latency_p50_ms,latency_p95_ms,latency_p99_ms,rtf_avg")
for key in sorted(groups):
    rows = groups[key]
    latencies = [float(row["latency_ms"]) for row in rows]
    rtfs = [float(row["rtf"]) for row in rows if row["rtf"]]
    lines.append(
        ",".join(
            [
                *key,
                str(len(rows)),
                f"{statistics.median(latencies):.3f}",
                f"{percentile(latencies, 95):.3f}",
                f"{percentile(latencies, 99):.3f}",
                f"{statistics.mean(rtfs):.6f}" if rtfs else "",
            ]
        )
    )

summary_path.write_text("\n".join(lines) + "\n")
print(summary_path.read_text())
PY

echo "Wrote ${CSV} and ${SUMMARY}"
