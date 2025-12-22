# Contributing

Thanks for helping improve mvx. This guide keeps contributions consistent and easy to review.

## Development Setup

- Rust toolchain: install via rustup.
- External tools for conversions: ImageMagick and ffmpeg/ffprobe.

Common commands:
- `cargo build`
- `cargo test`
- `cargo fmt`
- `cargo clippy`

## Code Style

- Use `cargo fmt` before pushing.
- Keep `cargo clippy -- -D warnings` clean.
- Prefer small, focused commits with Conventional Commit messages.

## Tests

- Unit tests live alongside modules.
- Integration tests are in `tests/`.
- Some integration tests skip when external tools are unavailable.

## Pull Requests

- Include a short summary and any relevant issue links.
- Add screenshots or CLI output for user-facing changes.
- Update `README.md` and `CHANGELOG.md` when behavior changes.
