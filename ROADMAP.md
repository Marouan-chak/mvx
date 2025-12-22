# Roadmap

These are the next improvements planned for mvx, ordered by impact.

## 1. Config + Profiles

- Add `~/.config/mvx/config.toml` (or `XDG_CONFIG_HOME`) for default options.
- Support named profiles (e.g., `--profile high`).

## 2. Batch Mode

- Accept multiple inputs (globs, directories, stdin list).
- Provide a summary report and failure count.

## 3. Progress UX + JSON Output

- Single-line progress updates, optional JSON status for automation.
- Cleaner stderr output from external tools.

## 4. Detection and Planning

- Use `file` as a fallback to infer types.
- Add PDF page count for multi-page conversions.

## 5. Packaging

- AUR package, multi-platform release artifacts via `cargo-dist`.

## 6. Test Coverage

- Golden plan snapshots, mock tool integration tests for CI.
