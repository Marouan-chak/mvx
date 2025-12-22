#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/samples"

mkdir -p "${OUT_DIR}"

if command -v magick >/dev/null 2>&1; then
  magick -size 64x64 xc:skyblue "${OUT_DIR}/input.png"
elif command -v convert >/dev/null 2>&1; then
  convert -size 64x64 xc:skyblue "${OUT_DIR}/input.png"
else
  echo "ImageMagick not found; skipping image sample" >&2
fi

if command -v ffmpeg >/dev/null 2>&1; then
  ffmpeg -y -f lavfi -i "sine=frequency=1000:duration=0.5" "${OUT_DIR}/input.wav"
  ffmpeg -y -f lavfi -i "testsrc=size=128x72:rate=15" \
    -f lavfi -i "sine=frequency=500:duration=0.5" \
    -shortest -c:v libx264 -pix_fmt yuv420p -c:a aac -b:a 96k \
    "${OUT_DIR}/input.mp4"
else
  echo "ffmpeg not found; skipping audio/video samples" >&2
fi

echo "Samples written to ${OUT_DIR}"
