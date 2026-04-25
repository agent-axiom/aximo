#!/usr/bin/env bash
set -euo pipefail

RESULTS_CSV="${AXIMO_BENCH_RESULTS_CSV:-target/aximo-benchmarks/results.csv}"
SUMMARY_CSV="${AXIMO_BENCH_SUMMARY:-target/aximo-benchmarks/summary.txt}"
REPORT_PATH="${AXIMO_BENCH_REPORT:-target/aximo-benchmarks/benchmark-report.md}"
REPORT_TITLE="${AXIMO_BENCH_REPORT_TITLE:-Aximo STT Benchmark Report}"
FIXTURE_SET="${AXIMO_BENCH_FIXTURE_SET:-unversioned}"
MODEL_SET="${AXIMO_BENCH_MODEL_SET:-configured service models}"
HOST_INFO="${AXIMO_BENCH_HOST_INFO:-unknown host}"
NOTES="${AXIMO_BENCH_NOTES:-}"

if [ ! -f "$RESULTS_CSV" ]; then
    echo "benchmark results CSV not found: $RESULTS_CSV" >&2
    exit 2
fi

mkdir -p "$(dirname "$REPORT_PATH")"

python3 - "$RESULTS_CSV" "$SUMMARY_CSV" "$REPORT_PATH" "$REPORT_TITLE" "$FIXTURE_SET" "$MODEL_SET" "$HOST_INFO" "$NOTES" <<'PY'
import collections
import csv
import datetime as dt
import pathlib
import sys

results_path = pathlib.Path(sys.argv[1])
summary_path = pathlib.Path(sys.argv[2])
report_path = pathlib.Path(sys.argv[3])
title, fixture_set, model_set, host_info, notes = sys.argv[4:9]

with results_path.open(newline="") as fh:
    rows = list(csv.DictReader(fh))

status_counts = collections.Counter(row.get("http_status", "") for row in rows)
error_counts = collections.Counter(
    (row.get("http_status", ""), row.get("error_code", ""))
    for row in rows
    if row.get("http_status") != "200"
)
warmups = sum(1 for row in rows if row.get("run", "").startswith("warmup"))
measured = len(rows) - warmups
quality_rows = [
    row
    for row in rows
    if row.get("http_status") == "200"
    and not row.get("run", "").startswith("warmup")
    and row.get("wer")
    and row.get("cer")
]

def markdown_table_from_csv(path):
    if not path.exists():
        return "_No summary file was generated._\n"
    with path.open(newline="") as fh:
        summary_rows = list(csv.reader(fh))
    if not summary_rows:
        return "_No successful benchmark rows were aggregated._\n"
    header, *body = summary_rows
    lines = [
        "| " + " | ".join(header) + " |",
        "| " + " | ".join(["---"] * len(header)) + " |",
    ]
    for row in body:
        lines.append("| " + " | ".join(row) + " |")
    return "\n".join(lines) + "\n"

def status_table():
    if not status_counts:
        return "_No benchmark rows found._\n"
    lines = ["| http_status | rows |", "| --- | ---: |"]
    for status, count in sorted(status_counts.items()):
        lines.append(f"| {status or 'missing'} | {count} |")
    return "\n".join(lines) + "\n"

def error_table():
    if not error_counts:
        return "_No non-200 benchmark rows found._\n"
    lines = ["| http_status | error_code | rows |", "| --- | --- | ---: |"]
    for (status, code), count in sorted(error_counts.items()):
        lines.append(f"| {status or 'missing'} | {code or 'missing'} | {count} |")
    return "\n".join(lines) + "\n"

quality_note = (
    f"{len(quality_rows)} successful measured rows include WER/CER from transcript sidecars."
    if quality_rows
    else "No successful rows include WER/CER. Use AXIMO_BENCH_FIXTURES_DIR with .txt transcript sidecars for quality evidence."
)

generated_at = dt.datetime.now(dt.timezone.utc).isoformat()
contents = f"""# {title}

Generated at: `{generated_at}`

## Scope

- Results CSV: `{results_path}`
- Summary CSV: `{summary_path}`
- Fixture set: `{fixture_set}`
- Model set: `{model_set}`
- Host info: `{host_info}`
- Warmup rows: `{warmups}`
- Measured rows: `{measured}`
- Quality rows: `{len(quality_rows)}`

{notes.strip() if notes.strip() else "_No additional notes supplied._"}

## Aggregated Results

{markdown_table_from_csv(summary_path)}

## Status Counts

{status_table()}

## Error Counts

{error_table()}

## Quality Coverage

{quality_note}

## Reproduction

Run the service with the target model and CPU/RAM limits, then run:

```bash
AXIMO_BENCH_FIXTURES_DIR=<real-speech-fixtures> \\
AXIMO_BENCH_EXPECTED_DIR=<matching-transcripts> \\
AXIMO_BENCH_RUNS=10 \\
AXIMO_BENCH_WARMUPS=2 \\
just benchmark-api
```

The benchmark harness writes `results.csv`, `summary.txt`, and this Markdown report under the benchmark output directory by default.
"""

report_path.write_text(contents)
print(f"Wrote {report_path}")
PY
