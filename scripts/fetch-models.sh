#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/fetch-models.sh [destination-dir]

Downloads the Parakeet v3 int8 ONNX bundle used by aximo/transcribe-rs.

Arguments:
  destination-dir    Target models directory.
                     Default: ./var/models

Environment:
  AXIMO_MODELS_DIR   Overrides the destination directory.
  AXIMO_PARAKEET_URL Overrides the Parakeet archive URL.
  AXIMO_FORCE=1      Re-download even if the model already exists.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

if ! command -v tar >/dev/null 2>&1; then
  echo "tar is required" >&2
  exit 1
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
MODELS_DIR="${1:-${AXIMO_MODELS_DIR:-${REPO_ROOT}/var/models}}"
PARAKEET_URL="${AXIMO_PARAKEET_URL:-https://blob.handy.computer/parakeet-v3-int8.tar.gz}"
TARGET_DIR="${MODELS_DIR}/parakeet-tdt-0.6b-v3-int8"
FORCE_DOWNLOAD="${AXIMO_FORCE:-0}"

if [[ -d "${TARGET_DIR}" && "${FORCE_DOWNLOAD}" != "1" ]]; then
  echo "Parakeet model already present at ${TARGET_DIR}"
  exit 0
fi

tmp_dir="$(mktemp -d)"
archive_path="${tmp_dir}/parakeet-v3-int8.tar.gz"
extract_dir="${tmp_dir}/extract"

cleanup() {
  rm -rf "${tmp_dir}"
}

trap cleanup EXIT

mkdir -p "${MODELS_DIR}" "${extract_dir}"

echo "Downloading Parakeet model from ${PARAKEET_URL}"
curl -fL "${PARAKEET_URL}" -o "${archive_path}"

echo "Extracting model archive"
tar -xzf "${archive_path}" -C "${extract_dir}"

source_dir=""
if [[ -d "${extract_dir}/parakeet-tdt-0.6b-v3-int8" ]]; then
  source_dir="${extract_dir}/parakeet-tdt-0.6b-v3-int8"
else
  for candidate in "${extract_dir}"/*; do
    if [[ -d "${candidate}" && -f "${candidate}/vocab.txt" ]]; then
      source_dir="${candidate}"
      break
    fi
  done
fi

if [[ -z "${source_dir}" ]]; then
  echo "Unable to locate extracted model directory" >&2
  exit 1
fi

rm -rf "${TARGET_DIR}"
mkdir -p "${TARGET_DIR}"
cp -R "${source_dir}/." "${TARGET_DIR}/"

for required in \
  encoder-model.int8.onnx \
  decoder_joint-model.int8.onnx \
  nemo128.onnx \
  vocab.txt
do
  if [[ ! -f "${TARGET_DIR}/${required}" ]]; then
    echo "Missing required model file: ${TARGET_DIR}/${required}" >&2
    exit 1
  fi
done

echo "Model ready at ${TARGET_DIR}"
