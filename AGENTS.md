# Repository Guidelines

## Project Overview
mvx is a planned Linux CLI that combines rename and format conversion into one action. The repository currently contains design and technical notes; implementation is expected in Rust with external tooling (ffmpeg, ImageMagick, LibreOffice headless).

## Project Structure & Module Organization
- `src/main.rs`: CLI entry point and argument parsing.
- `src/plan.rs`: Plan model and strategy selection.
- `src/execute.rs`: Execution pipeline for rename/copy operations.
- `src/detect.rs`: File type sniffing helpers.
- `src/ffprobe.rs`: ffprobe wrapper for media inspection.
- `src/config.rs`: Config file and profile loading.
- `src/batch.rs`: Batch input collection and destination mapping.
- `tests/conversion.rs`: Integration tests for ImageMagick and ffmpeg conversions.
- `presentation.md`: Product overview, behavior, and UX requirements.
- `tech_choices.md`: Proposed Rust crates and external toolchain.
- `README.md`: Developer and usage guide.

## Build, Test, and Development Commands
- `cargo build`: Compile the CLI locally.
- `cargo run -- <args>`: Run the CLI with arguments.
- `cargo test`: Run unit/integration tests.
- `cargo fmt`: Format Rust code.
- `cargo clippy`: Lint Rust code.
- `mvx` conversions require external tools installed (ImageMagick for images, ffmpeg/ffprobe for audio/video, LibreOffice for documents).

## Coding Style & Naming Conventions
- Rust: 4-space indentation, `snake_case` for functions/modules, `CamelCase` for types, and `SCREAMING_SNAKE_CASE` for constants.
- Prefer `cargo fmt` and `cargo clippy` once a Rust workspace exists.
- CLI flags should be kebab-case (e.g., `--dry-run`, `--overwrite`).

## Testing Guidelines
- Uses Rust’s built-in test harness.
- Unit tests live alongside code (see `src/plan.rs`).
- Integration tests should go under `tests/` as they are added.
- Name test files after the feature area (e.g., `tests/plan.rs`).
- Run with `cargo test`.

## Commit & Pull Request Guidelines
No commit history exists yet, so no established convention. Suggested starting point:
- Commits: short, imperative subject lines (e.g., `Add plan builder`).
- PRs: include a brief description, linked issue (if any), and CLI output or screenshots for UX changes.

## Architecture Overview
mvx is designed as a plan-first CLI:
- Parse CLI flags and paths, then sniff the real input type.
- Build a conversion plan (rename, remux, transcode, convert) with chosen backend.
- Execute the plan, stream progress, validate output, and finalize with an atomic rename.
- Prefer safe writes via a temp file in the destination directory.

## Security & Configuration Tips
- mvx should avoid destructive operations by default; document any flags that delete or overwrite.
- Treat external tool invocation as untrusted input; validate paths and handle failures with clear errors.
