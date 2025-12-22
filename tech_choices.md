Tech stack for mvx

Language and packaging

* Rust
* Single static style binary distribution, using cargo dist for releases

CLI and UX

* clap for argument parsing and help output
* indicatif for progress bars and spinners
* console for terminal formatting, optional
* dialoguer for interactive prompts, optional

Errors and logging

* anyhow for ergonomic error handling
* thiserror for structured error types
* tracing and tracing subscriber for debug logging, optional

Type detection and metadata

* infer for fast file type sniffing from file bytes
* serde and serde json for parsing ffprobe JSON and for plan output

Safe file operations

* tempfile for temp outputs in the destination directory
* std fs for atomic rename and file operations
* optional fsync support for stronger durability

Converters and external tools

* ffmpeg plus ffprobe for audio and video conversion, remux, and progress reporting
* ImageMagick for image conversions
* LibreOffice headless for document to pdf conversions

