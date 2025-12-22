#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SAMPLES_DIR="${ROOT_DIR}/samples"

if [[ ! -d "${SAMPLES_DIR}" ]]; then
  echo "samples/ not found. Run scripts/make-samples.sh first." >&2
  exit 1
fi

run() {
  echo "+ $*"
  "$@"
}

# Plan checks
run cargo run -- --plan "${SAMPLES_DIR}/input.png" "${SAMPLES_DIR}/output.jpg"
run cargo run -- --plan "${SAMPLES_DIR}/input.wav" "${SAMPLES_DIR}/output.flac"
run cargo run -- --plan "${SAMPLES_DIR}/input.mp4" "${SAMPLES_DIR}/output.webm"
run cargo run -- --plan "${SAMPLES_DIR}/input.txt" "${SAMPLES_DIR}/output.pdf"

# Image conversion
if [[ -f "${SAMPLES_DIR}/input.png" ]]; then
  run cargo run -- "${SAMPLES_DIR}/input.png" "${SAMPLES_DIR}/output.jpg"
fi

# Audio conversion
if [[ -f "${SAMPLES_DIR}/input.wav" ]]; then
  run cargo run -- --audio-bitrate 192k "${SAMPLES_DIR}/input.wav" "${SAMPLES_DIR}/output.flac"
fi

# Video conversion
if [[ -f "${SAMPLES_DIR}/input.mp4" ]]; then
  run cargo run -- --transcode "${SAMPLES_DIR}/input.mp4" "${SAMPLES_DIR}/output.webm"
  run cargo run -- --stream-copy "${SAMPLES_DIR}/input.mp4" "${SAMPLES_DIR}/output.mov"
fi

if [[ -f "${SAMPLES_DIR}/input.txt" ]]; then
  run cargo run -- "${SAMPLES_DIR}/input.txt" "${SAMPLES_DIR}/output.pdf"
fi

echo "mvx sample tests completed."
