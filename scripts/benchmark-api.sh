#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${AXIMO_BENCH_BASE_URL:-http://127.0.0.1:8080}"
OUT_DIR="${AXIMO_BENCH_OUT_DIR:-target/aximo-benchmarks}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENGINES="${AXIMO_BENCH_ENGINES:-parakeet}"
FORMATS="${AXIMO_BENCH_FORMATS:-wav}"
DURATIONS="${AXIMO_BENCH_DURATIONS:-5 30 60}"
RUNS="${AXIMO_BENCH_RUNS:-5}"
WARMUPS="${AXIMO_BENCH_WARMUPS:-1}"
LANGUAGE="${AXIMO_BENCH_LANGUAGE:-}"
TIMESTAMPS="${AXIMO_BENCH_TIMESTAMPS:-false}"
FIXTURES_DIR="${AXIMO_BENCH_FIXTURES_DIR:-}"
EXPECTED_DIR="${AXIMO_BENCH_EXPECTED_DIR:-}"

mkdir -p "$OUT_DIR/audio" "$OUT_DIR/responses"

CSV="$OUT_DIR/results.csv"
SUMMARY="$OUT_DIR/summary.txt"
REPORT="${AXIMO_BENCH_REPORT:-$OUT_DIR/benchmark-report.md}"

cat > "$CSV" <<'CSV'
timestamp,engine,format,duration_s,sample,run,http_status,latency_ms,processing_ms,duration_ms,rtf,text_chars,expected_chars,wer,cer,error_code
CSV

if [ -z "$FIXTURES_DIR" ]; then
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

duration_for() {
    local input="$1"
    if command -v ffprobe >/dev/null 2>&1; then
        ffprobe -v error -show_entries format=duration -of default=nokey=1:noprint_wrappers=1 "$input" \
            | awk '{printf "%.3f", $1}'
    else
        printf ""
    fi
}

run_once() {
    local engine="$1"
    local format="$2"
    local duration_s="$3"
    local run_id="$4"
    local input="$5"
    local sample="$6"
    local expected_path="$7"
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

    python3 - "$CSV" "$response" "$curl_out" "$engine" "$format" "$duration_s" "$sample" "$run_id" "$expected_path" <<'PY'
import csv
import datetime as dt
import json
import pathlib
import re
import sys

csv_path = pathlib.Path(sys.argv[1])
response_path = pathlib.Path(sys.argv[2])
http_status, latency_seconds = sys.argv[3].split()
engine, fmt, duration_s, sample, run_id, expected_path = sys.argv[4:10]

try:
    payload = json.loads(response_path.read_text())
except json.JSONDecodeError:
    payload = {}

processing_ms = payload.get("processing_ms", "")
duration_ms = payload.get("duration_ms", "")
text = payload.get("text", "")
error_code = payload.get("code", "")
expected_text = ""
if expected_path:
    path = pathlib.Path(expected_path)
    if path.exists():
        expected_text = path.read_text().strip()
rtf = ""
if isinstance(processing_ms, int) and isinstance(duration_ms, int) and duration_ms > 0:
    rtf = f"{processing_ms / duration_ms:.6f}"

def edit_distance(left, right):
    previous = list(range(len(right) + 1))
    for i, left_item in enumerate(left, 1):
        current = [i]
        for j, right_item in enumerate(right, 1):
            current.append(
                min(
                    previous[j] + 1,
                    current[j - 1] + 1,
                    previous[j - 1] + (left_item != right_item),
                )
            )
        previous = current
    return previous[-1]

def normalize_words(value):
    return re.findall(r"[\w']+", value.lower(), flags=re.UNICODE)

def normalize_chars(value):
    return list(" ".join(normalize_words(value)))

wer = ""
cer = ""
if expected_text and isinstance(text, str):
    expected_words = normalize_words(expected_text)
    actual_words = normalize_words(text)
    if expected_words:
        wer = f"{edit_distance(expected_words, actual_words) / len(expected_words):.6f}"
    expected_chars = normalize_chars(expected_text)
    actual_chars = normalize_chars(text)
    if expected_chars:
        cer = f"{edit_distance(expected_chars, actual_chars) / len(expected_chars):.6f}"

row = {
    "timestamp": dt.datetime.now(dt.timezone.utc).isoformat(),
    "engine": engine,
    "format": fmt,
    "duration_s": duration_s,
    "sample": sample,
    "run": run_id,
    "http_status": http_status,
    "latency_ms": f"{float(latency_seconds) * 1000:.3f}",
    "processing_ms": processing_ms,
    "duration_ms": duration_ms,
    "rtf": rtf,
    "text_chars": len(text) if isinstance(text, str) else "",
    "expected_chars": len(expected_text),
    "wer": wer,
    "cer": cer,
    "error_code": error_code,
}

with csv_path.open("a", newline="") as fh:
    writer = csv.DictWriter(fh, fieldnames=row.keys())
    writer.writerow(row)
PY
}

echo "Benchmarking ${BASE_URL}; results: ${CSV}"

if [ -n "$FIXTURES_DIR" ]; then
    if [ ! -d "$FIXTURES_DIR" ]; then
        echo "AXIMO_BENCH_FIXTURES_DIR does not exist: $FIXTURES_DIR" >&2
        exit 2
    fi
    mapfile -t FIXTURES < <(find "$FIXTURES_DIR" -type f \( -iname '*.wav' -o -iname '*.mp3' -o -iname '*.flac' -o -iname '*.m4a' \) | sort)
    if [ "${#FIXTURES[@]}" -eq 0 ]; then
        echo "no wav/mp3/flac/m4a fixtures found in $FIXTURES_DIR" >&2
        exit 2
    fi

    for engine in $ENGINES; do
        for input in "${FIXTURES[@]}"; do
            filename="$(basename "$input")"
            sample="${filename%.*}"
            format="${filename##*.}"
            format="$(printf "%s" "$format" | tr '[:upper:]' '[:lower:]')"
            duration_s="$(duration_for "$input")"
            expected_path="${input%.*}.txt"
            if [ -n "$EXPECTED_DIR" ]; then
                expected_path="${EXPECTED_DIR}/${sample}.txt"
            fi
            for warmup in $(seq 1 "$WARMUPS"); do
                run_once "$engine" "$format" "$duration_s" "warmup-${warmup}" "$input" "$sample" "$expected_path" >/dev/null
            done
            for run in $(seq 1 "$RUNS"); do
                run_once "$engine" "$format" "$duration_s" "$run" "$input" "$sample" "$expected_path"
            done
        done
    done
else
    for engine in $ENGINES; do
        for format in $FORMATS; do
            for duration_s in $DURATIONS; do
                input="$OUT_DIR/audio/tone-${duration_s}s.${format}"
                sample="tone-${duration_s}s"
                for warmup in $(seq 1 "$WARMUPS"); do
                    run_once "$engine" "$format" "$duration_s" "warmup-${warmup}" "$input" "$sample" "" >/dev/null
                done
                for run in $(seq 1 "$RUNS"); do
                    run_once "$engine" "$format" "$duration_s" "$run" "$input" "$sample" ""
                done
            done
        done
    done
fi

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
        key = (row["engine"], row["format"], row["duration_s"], row["sample"])
        groups[key].append(row)

def percentile(values, pct):
    ordered = sorted(values)
    if not ordered:
        return 0.0
    index = min(len(ordered) - 1, round((pct / 100) * (len(ordered) - 1)))
    return ordered[index]

lines = []
lines.append("engine,format,duration_s,sample,runs,latency_p50_ms,latency_p95_ms,latency_p99_ms,rtf_avg,wer_avg,cer_avg")
for key in sorted(groups):
    rows = groups[key]
    latencies = [float(row["latency_ms"]) for row in rows]
    rtfs = [float(row["rtf"]) for row in rows if row["rtf"]]
    wers = [float(row["wer"]) for row in rows if row["wer"]]
    cers = [float(row["cer"]) for row in rows if row["cer"]]
    lines.append(
        ",".join(
            [
                *key,
                str(len(rows)),
                f"{statistics.median(latencies):.3f}",
                f"{percentile(latencies, 95):.3f}",
                f"{percentile(latencies, 99):.3f}",
                f"{statistics.mean(rtfs):.6f}" if rtfs else "",
                f"{statistics.mean(wers):.6f}" if wers else "",
                f"{statistics.mean(cers):.6f}" if cers else "",
            ]
        )
    )

summary_path.write_text("\n".join(lines) + "\n")
print(summary_path.read_text())
PY

AXIMO_BENCH_RESULTS_CSV="$CSV" \
    AXIMO_BENCH_SUMMARY="$SUMMARY" \
    AXIMO_BENCH_REPORT="$REPORT" \
    "$SCRIPT_DIR/render-benchmark-report.sh"

echo "Wrote ${CSV}, ${SUMMARY}, and ${REPORT}"
