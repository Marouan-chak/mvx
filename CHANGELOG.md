# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- JSON output for plans and batch summaries.

## [0.1.5] - 2025-12-22

### Added
- Batch mode for multiple inputs (directories, globs, stdin).

## [0.1.4] - 2025-12-22

### Added
- Config file support with profiles (XDG config path).

## [0.1.3] - 2025-12-22

### Added
- PDF/image conversions via ImageMagick (first page for PDF input).
- ImageMagick PDF capability checks in integration tests.

## [0.1.2] - 2025-12-22

### Added
- Document conversion via LibreOffice (`doc/docx/ppt/pptx/xls/xlsx/odt/odp/ods/rtf/txt → pdf`).
- Sample generation and test scripts for local validation.

## [0.1.1] - 2025-12-22

### Added
- ffprobe-based stream-copy decisions with plan previews.
- Default codec mappings and ffmpeg option flags.
- Progress output improvements and backup mode.
- Integration tests for stream-copy and codec flags.

## [0.1.0] - 2025-12-22

### Added
- Initial CLI with plan output, safe writes, and basic conversions.
