# mvx

mvx is a Linux CLI that combines renaming and conversion into one action. You pass a source path and a destination path; mvx detects the real input type, selects a strategy (rename, copy, or convert), and performs a safe output write.

## Quick Start

Examples:
- Rename only: `mvx photo.jpeg photo.jpg`
- Convert image: `mvx image.png image.jpg`
- Convert audio: `mvx input.wav output.flac`
- Convert video: `mvx clip.mov clip.mp4`
- Show plan only: `mvx --plan input.png output.jpg`
  - Plan output includes backend selection, ffmpeg mode, and a command preview.

## Usage and Options

Basic form:
```
mvx <source> <destination> [--plan|--dry-run] [--overwrite|--backup] [--move-source]
```
`--stream-copy` and `--transcode` are mutually exclusive.
`--overwrite` and `--backup` are mutually exclusive.

Conversion tuning:
- `--image-quality <1-100>`: ImageMagick quality for image conversions.
- `--video-bitrate <n[k|m]>`: Target video bitrate for ffmpeg conversions (e.g., `2500k`).
- `--audio-bitrate <n[k|m]>`: Target audio bitrate for ffmpeg conversions (e.g., `192k`).
- `--preset <name>`: ffmpeg preset for video conversions (ultrafast..veryslow).
- `--video-codec <name>`: ffmpeg video codec (e.g., `libx264`, `libx265`, `vp9`).
- `--audio-codec <name>`: ffmpeg audio codec (e.g., `aac`, `libopus`, `flac`).
- `--stream-copy`: Force ffmpeg stream copy (no re-encode) when possible.
- `--transcode`: Force ffmpeg re-encode.
- `--backup`: If destination exists, move it to `*.bak` (or `*.bak.N`) before writing.

Options are validated and ignored when they do not apply (for example, `--video-bitrate` on audio-only outputs).

## Dependencies

mvx shells out to external tools for conversions:
- ImageMagick (`magick` or `convert`) for images
- ffmpeg for audio and video
- ffprobe for media inspection and stream-copy decisions

If a required tool is missing, mvx fails with an install hint. ffprobe is optional; without it mvx falls back to transcode.

## Safety Guarantees

- Output is written to a temporary file in the destination directory.
- Output must be non-empty before it is finalized.
- Finalization uses an atomic rename.
- Source files are kept by default; use `--move-source` to delete after success.
- Destination is not overwritten unless `--overwrite` is passed.
- `--backup` preserves existing destinations with a `.bak` suffix before writing.

## Conversion Behavior

- For media conversions, mvx may use ffprobe to decide whether stream-copy/remux is possible.
- When stream-copy is used, no re-encoding happens and conversions are much faster.
- ffmpeg progress is parsed and reported as a percentage with ETA when duration is known.
- When duration is unknown, progress shows elapsed seconds instead.
- Auto stream-copy compatibility targets:
  - `mp4`/`mov`: h264/hevc/mpeg4/av1 video with aac/mp3/alac audio.
  - `webm`: vp8/vp9/av1 video with opus/vorbis audio.
  - `mkv`: stream-copy allowed for most codecs.
- Default transcode codecs when not specified:
  - `mp4`/`mov`: `libx264` + `aac`
  - `webm`: `libvpx-vp9` + `libopus`
  - `mkv`/`avi`: `libx264` + `aac`
  - audio outputs: `mp3`→`libmp3lame`, `flac`→`flac`, `wav`→`pcm_s16le`, `opus`→`libopus`, `ogg`→`libvorbis`, `m4a`/`aac`→`aac`

## Development

Common commands:
- `cargo build`
- `cargo run -- <args>`
- `cargo test`
- `cargo fmt`
- `cargo clippy`

Integration tests in `tests/conversion.rs` automatically skip when external tools are unavailable.

## Release

Tagging a version triggers the GitHub Actions release workflow:
- `git tag v0.1.0`
- `git push origin v0.1.0`

The release uploads the Linux `mvx` binary and a SHA-256 checksum.
