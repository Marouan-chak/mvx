```markdown
# mvx

mvx is a Linux CLI that combines rename and conversion into a single action.

You provide a source path and a destination path. mvx compares what you have with what you asked for. If it is a plain rename it performs a rename. If the destination extension implies a different format, mvx performs a real conversion and writes the destination file safely.

## Goals

- Feel like a native Linux tool
- Be fast in the common cases
- Be predictable and safe with user data
- Make it obvious what is happening through a clear plan and progress output
- Ship as a single binary

## What mvx does

### 1. Rename only when no conversion is needed

mvx will do a normal rename when the change is only cosmetic or equivalent.

Examples
- `mvx photo.jpeg photo.jpg`
- `mvx notes.htm notes.html`

### 2. Convert when the destination extension implies a new format

mvx treats a destination extension change as an explicit request to convert.

Examples
- `mvx photo.png photo.jpg`
- `mvx audio.wav audio.mp3`
- `mvx clip.mov clip.mp4`
- `mvx report.docx report.pdf`

### 3. Prefer fast paths when possible

mvx attempts the fastest correct strategy first.

Decision order
1. Already in target format, rename only
2. Container change only, remux or stream copy
3. Re encode or full conversion

Example
- mov to mp4 with compatible codecs can be a remux, which is much faster than transcoding

## User experience

### Plan and dry run

mvx always computes a conversion plan. You can ask it to show the plan without doing any work.

Plan output includes
- Detected input type, based on file content
- Requested output type, based on destination extension
- Chosen strategy, rename, remux, transcode, convert
- Key defaults that affect output, for example quality, bitrate, preset
- Safety behavior, for example whether source is kept, overwrite rules
- Destination path

### Progress reporting

mvx shows progress during execution so users know it is working.

Progress style by backend
- Media conversions use structured ffmpeg progress and display percent, speed, and eta when available
- Conversions that do not expose progress use a spinner plus stage text and elapsed time

### Data safety

mvx prioritizes safety without adding much overhead.

Default safety rules
- Source is never deleted by default
- Output is written to a temporary file in the destination directory
- Output is validated before it replaces anything
- Finalization uses an atomic rename so the destination is either correct or unchanged
- If the destination exists, mvx refuses by default unless an explicit overwrite option is used
- Optional backups can be enabled when overwriting

## Technical approach

### Platform and language

- Target platform: Linux
- Implementation language: Rust
- Distribution: single binary

### Core architecture

mvx is a smart router around proven converters. It does not attempt to re implement codecs or document renderers.

Pipeline
1. Parse CLI arguments and flags
2. Inspect the source file and detect real type from bytes
3. Build a plan that decides the strategy and selects a backend
4. Execute the plan
5. Stream progress to the terminal
6. Validate output
7. Atomically finalize the destination

### Detection and probing

- Use fast byte sniffing to detect actual file type rather than trusting extensions
- For audio and video, use ffprobe JSON output to obtain codec and duration data for decision making and progress

### Backends

mvx selects a backend based on source type and requested output.

Typical backends
- ffmpeg and ffprobe for audio and video
- ImageMagick for images
- LibreOffice headless for office documents to pdf
- Optional future backends for ebooks and archives

### Strategy selection for speed

mvx chooses the least expensive correct strategy.

Examples
- rename only if the content already matches the target
- remux when only the container changes and codecs can be copied
- transcode only when necessary

Media example strategies
- remux or stream copy using ffmpeg with codec copy
- transcode with a fast preset when copy is not possible

### Safe writes and atomic finalization

mvx uses safe filesystem practices to avoid partial outputs and accidental data loss.

Key techniques
- temporary output file in the destination directory
- validate that output exists and is non zero
- atomic rename into place
- optional stronger durability, such as fsync of the file and parent directory, can be added as a mode when needed

## MVP scope

Recommended initial scope
- Audio and video conversions via ffmpeg
- Image conversions via ImageMagick
- Basic office document to pdf via LibreOffice headless

Everything else is treated as best effort and added via backend plugins over time.

## Non goals

- Guaranteeing perfect conversion between every possible pair of formats
- Replacing specialist tools for niche or proprietary formats
- Inferring user intent from normal mv usage, mvx is explicit by design
```

