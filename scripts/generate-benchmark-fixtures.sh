#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${AXIMO_BENCH_FIXTURE_OUT_DIR:-target/aximo-benchmarks/fixtures}"
FORMATS="${AXIMO_BENCH_FIXTURE_FORMATS:-wav}"
EN_VOICE="${AXIMO_BENCH_EN_VOICE:-Samantha}"
RU_VOICE="${AXIMO_BENCH_RU_VOICE:-Milena}"

if ! command -v say >/dev/null 2>&1; then
    echo "macOS 'say' is required to generate local speech fixtures" >&2
    exit 2
fi

if ! say -v '?' | awk '{print $1}' | grep -qx "$EN_VOICE"; then
    echo "English voice not available: $EN_VOICE" >&2
    exit 2
fi

if ! say -v '?' | awk '{print $1}' | grep -qx "$RU_VOICE"; then
    echo "Russian voice not available: $RU_VOICE" >&2
    exit 2
fi

for format in $FORMATS; do
    case "$format" in
        wav)
            ;;
        mp3 | m4a | flac)
            if ! command -v ffmpeg >/dev/null 2>&1; then
                echo "ffmpeg is required for $format fixture generation" >&2
                exit 2
            fi
            ;;
        *)
            echo "unsupported fixture format: $format" >&2
            exit 2
            ;;
    esac
done

mkdir -p "$OUT_DIR"

write_sample() {
    local name="$1"
    local voice="$2"
    local text="$3"
    local wav_path="$OUT_DIR/${name}.wav"

    printf "%s\n" "$text" > "$OUT_DIR/${name}.txt"
    say -v "$voice" -o "$wav_path" --data-format=LEI16@16000 "$text"
    python3 - "$wav_path" <<'PY'
import pathlib
import sys
import wave

path = pathlib.Path(sys.argv[1])
try:
    with wave.open(str(path), "rb") as wav:
        frames = wav.getnframes()
        sample_rate = wav.getframerate()
except Exception as error:
    raise SystemExit(f"generated WAV is not readable: {path}: {error}")

if frames <= 0 or sample_rate <= 0:
    raise SystemExit(f"generated WAV contains no audio frames: {path}")
PY

    for format in $FORMATS; do
        case "$format" in
            wav)
                ;;
            mp3)
                ffmpeg -hide_banner -loglevel error -y -i "$wav_path" "$OUT_DIR/${name}.mp3"
                ;;
            m4a)
                ffmpeg -hide_banner -loglevel error -y -i "$wav_path" -c:a aac "$OUT_DIR/${name}.m4a"
                ;;
            flac)
                ffmpeg -hide_banner -loglevel error -y -i "$wav_path" "$OUT_DIR/${name}.flac"
                ;;
        esac
    done
}

write_sample \
    "en-tts-5s" \
    "$EN_VOICE" \
    "Aximo records short English speech for a local speech to text benchmark."

write_sample \
    "ru-tts-5s" \
    "$RU_VOICE" \
    "Аксимо записывает короткую русскую речь для локального теста распознавания."

echo "Wrote benchmark speech fixtures to $OUT_DIR"
echo "Run: AXIMO_BENCH_FIXTURES_DIR=$OUT_DIR just benchmark-api"
