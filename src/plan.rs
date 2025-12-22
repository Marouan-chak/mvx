use crate::detect::{DetectedType, detect_path};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    RenameOnly,
    CopyOnly,
    Convert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    ImageMagick,
    Ffmpeg,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub detected: DetectedType,
    pub strategy: Strategy,
    pub backend: Option<Backend>,
    pub notes: Vec<String>,
    pub move_source: bool,
    pub options: ConversionOptions,
    pub dest_ext: Option<String>,
    pub dest_kind: MediaKind,
}

#[derive(Debug, Clone)]
pub struct ConversionOptions {
    pub image_quality: Option<u8>,
    pub video_bitrate: Option<String>,
    pub audio_bitrate: Option<String>,
    pub preset: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub ffmpeg_preference: FfmpegPreference,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            image_quality: None,
            video_bitrate: None,
            audio_bitrate: None,
            preset: None,
            video_codec: None,
            audio_codec: None,
            ffmpeg_preference: FfmpegPreference::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegPreference {
    Auto,
    StreamCopy,
    Transcode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegMode {
    StreamCopy,
    Transcode,
}

pub fn build_plan(
    source: &Path,
    destination: &Path,
    move_source: bool,
    options: ConversionOptions,
) -> Result<Plan> {
    if source == destination {
        bail!("source and destination must differ");
    }

    let detected = detect_path(source);
    let source_ext = normalize_ext(source);
    let dest_ext = normalize_ext(destination);
    let dest_kind = classify_dest_kind(dest_ext.as_deref());

    validate_options(&options)?;

    let strategy = match (source_ext.as_deref(), dest_ext.as_deref()) {
        (Some(src), Some(dest)) if src == dest => {
            if move_source {
                Strategy::RenameOnly
            } else {
                Strategy::CopyOnly
            }
        }
        _ => Strategy::Convert,
    };

    let backend = if strategy == Strategy::Convert {
        select_backend(source_ext.as_deref(), dest_ext.as_deref())
    } else {
        None
    };

    let mut notes = Vec::new();
    if strategy == Strategy::Convert {
        if backend.is_none() {
            notes.push("no supported backend found for this conversion".to_string());
        }
        if backend == Some(Backend::Ffmpeg) {
            notes.push(
                "ffprobe may be used at runtime to choose stream copy vs transcode".to_string(),
            );
        }
    }
    if !move_source {
        notes.push("source will be kept".to_string());
    }
    notes.extend(option_warnings(&options, dest_kind, backend));

    Ok(Plan {
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        detected,
        strategy,
        backend,
        notes,
        move_source,
        options,
        dest_ext,
        dest_kind,
    })
}

pub fn render_plan(plan: &Plan, overwrite: bool) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Source: {}", plan.source.display()));
    lines.push(format!("Destination: {}", plan.destination.display()));
    lines.push(format!(
        "Detected: {}",
        plan.detected.mime.as_deref().unwrap_or("unknown")
    ));
    if let Some(ext) = plan.detected.ext_hint.as_deref() {
        lines.push(format!("Detected extension: {}", ext));
    }
    lines.push(format!(
        "Strategy: {}",
        match plan.strategy {
            Strategy::RenameOnly => "rename",
            Strategy::CopyOnly => "copy",
            Strategy::Convert => "convert",
        }
    ));
    if let Some(ext) = plan.dest_ext.as_deref() {
        lines.push(format!("Destination extension: {}", ext));
    }
    if let Some(backend) = &plan.backend {
        lines.push(format!(
            "Backend: {}",
            match backend {
                Backend::ImageMagick => "imagemagick",
                Backend::Ffmpeg => "ffmpeg",
            }
        ));
    }
    lines.push(format!(
        "Destination kind: {}",
        match plan.dest_kind {
            MediaKind::Image => "image",
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
            MediaKind::Other => "other",
        }
    ));
    if let Some(quality) = plan.options.image_quality {
        lines.push(format!("Image quality: {}", quality));
    }
    if let Some(bitrate) = plan.options.video_bitrate.as_deref() {
        lines.push(format!("Video bitrate: {}", bitrate));
    }
    if let Some(bitrate) = plan.options.audio_bitrate.as_deref() {
        lines.push(format!("Audio bitrate: {}", bitrate));
    }
    if let Some(preset) = plan.options.preset.as_deref() {
        lines.push(format!("Preset: {}", preset));
    }
    if let Some(codec) = plan.options.video_codec.as_deref() {
        lines.push(format!("Video codec: {}", codec));
    }
    if let Some(codec) = plan.options.audio_codec.as_deref() {
        lines.push(format!("Audio codec: {}", codec));
    }
    if let Some(backend) = &plan.backend
        && *backend == Backend::Ffmpeg
    {
        lines.push(format!(
            "FFmpeg mode: {}",
            match plan.options.ffmpeg_preference {
                FfmpegPreference::Auto => "auto",
                FfmpegPreference::StreamCopy => "stream-copy",
                FfmpegPreference::Transcode => "transcode",
            }
        ));
    }
    lines.push(format!(
        "Overwrite: {}",
        if overwrite { "yes" } else { "no" }
    ));
    for note in &plan.notes {
        lines.push(format!("Note: {}", note));
    }

    lines.join("\n")
}

fn normalize_ext(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let normalized = match ext.as_str() {
        "jpeg" => "jpg",
        "htm" => "html",
        _ => ext.as_str(),
    };
    Some(normalized.to_string())
}

fn select_backend(source_ext: Option<&str>, dest_ext: Option<&str>) -> Option<Backend> {
    if is_image_ext(source_ext) && is_image_ext(dest_ext) {
        return Some(Backend::ImageMagick);
    }
    if is_media_ext(source_ext) && is_media_ext(dest_ext) {
        return Some(Backend::Ffmpeg);
    }
    None
}

fn is_image_ext(ext: Option<&str>) -> bool {
    matches!(
        ext,
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "heic" | "avif")
    )
}

fn is_media_ext(ext: Option<&str>) -> bool {
    matches!(
        ext,
        Some(
            "mp3"
                | "wav"
                | "flac"
                | "aac"
                | "ogg"
                | "m4a"
                | "opus"
                | "mp4"
                | "mov"
                | "mkv"
                | "webm"
                | "avi"
        )
    )
}

fn is_audio_ext(ext: Option<&str>) -> bool {
    matches!(
        ext,
        Some("mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "opus")
    )
}

fn is_video_ext(ext: Option<&str>) -> bool {
    matches!(ext, Some("mp4" | "mov" | "mkv" | "webm" | "avi"))
}

fn classify_dest_kind(ext: Option<&str>) -> MediaKind {
    if is_image_ext(ext) {
        MediaKind::Image
    } else if is_audio_ext(ext) {
        MediaKind::Audio
    } else if is_video_ext(ext) {
        MediaKind::Video
    } else {
        MediaKind::Other
    }
}

fn validate_options(options: &ConversionOptions) -> Result<()> {
    if let Some(quality) = options.image_quality
        && (quality == 0 || quality > 100)
    {
        bail!("image quality must be between 1 and 100");
    }
    if let Some(bitrate) = options.video_bitrate.as_deref() {
        validate_bitrate(bitrate).context("invalid video bitrate")?;
    }
    if let Some(bitrate) = options.audio_bitrate.as_deref() {
        validate_bitrate(bitrate).context("invalid audio bitrate")?;
    }
    if let Some(preset) = options.preset.as_deref() {
        let preset = preset.to_ascii_lowercase();
        let allowed = [
            "ultrafast",
            "superfast",
            "veryfast",
            "faster",
            "fast",
            "medium",
            "slow",
            "slower",
            "veryslow",
        ];
        if !allowed.contains(&preset.as_str()) {
            bail!(
                "preset must be one of: ultrafast, superfast, veryfast, faster, fast, medium, slow, slower, veryslow"
            );
        }
    }
    if let Some(codec) = options.video_codec.as_deref()
        && codec.trim().is_empty()
    {
        bail!("video codec must be a non-empty string");
    }
    if let Some(codec) = options.audio_codec.as_deref()
        && codec.trim().is_empty()
    {
        bail!("audio codec must be a non-empty string");
    }
    Ok(())
}

fn validate_bitrate(bitrate: &str) -> Result<()> {
    if bitrate.is_empty() {
        bail!("bitrate is empty");
    }
    let (value, suffix) = bitrate.split_at(bitrate.len().saturating_sub(1));
    let (digits, suffix) = if suffix.chars().all(|c| c.is_ascii_digit()) {
        (bitrate, "")
    } else {
        (value, suffix)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        bail!("bitrate must be numeric with optional k/m suffix");
    }
    if !suffix.is_empty() && !matches!(suffix, "k" | "K" | "m" | "M") {
        bail!("bitrate suffix must be k or m");
    }
    Ok(())
}

fn option_warnings(
    options: &ConversionOptions,
    dest_kind: MediaKind,
    backend: Option<Backend>,
) -> Vec<String> {
    let mut notes = Vec::new();
    if dest_kind != MediaKind::Image && options.image_quality.is_some() {
        notes.push("image quality ignored for non-image output".to_string());
    }
    if dest_kind == MediaKind::Audio {
        if options.video_bitrate.is_some() {
            notes.push("video bitrate ignored for audio-only output".to_string());
        }
        if options.preset.is_some() {
            notes.push("preset ignored for audio-only output".to_string());
        }
    }
    if dest_kind == MediaKind::Image && options.video_bitrate.is_some() {
        notes.push("video bitrate ignored for image output".to_string());
    }
    if dest_kind == MediaKind::Image && options.audio_bitrate.is_some() {
        notes.push("audio bitrate ignored for image output".to_string());
    }
    if dest_kind == MediaKind::Image && options.video_codec.is_some() {
        notes.push("video codec ignored for image output".to_string());
    }
    if dest_kind == MediaKind::Image && options.audio_codec.is_some() {
        notes.push("audio codec ignored for image output".to_string());
    }
    if dest_kind == MediaKind::Audio && options.video_codec.is_some() {
        notes.push("video codec ignored for audio-only output".to_string());
    }
    if backend != Some(Backend::Ffmpeg) && options.ffmpeg_preference != FfmpegPreference::Auto {
        notes.push("ffmpeg mode preference ignored for non-ffmpeg backend".to_string());
    }
    if options.ffmpeg_preference == FfmpegPreference::StreamCopy {
        if options.video_bitrate.is_some() {
            notes.push("video bitrate ignored when stream copy is forced".to_string());
        }
        if options.audio_bitrate.is_some() {
            notes.push("audio bitrate ignored when stream copy is forced".to_string());
        }
        if options.preset.is_some() {
            notes.push("preset ignored when stream copy is forced".to_string());
        }
        if options.video_codec.is_some() {
            notes.push("video codec ignored when stream copy is forced".to_string());
        }
        if options.audio_codec.is_some() {
            notes.push("audio codec ignored when stream copy is forced".to_string());
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_extension_aliases() {
        let jpeg = Path::new("photo.JPEG");
        let html = Path::new("index.HTM");
        let plain = Path::new("clip.mp4");

        assert_eq!(normalize_ext(jpeg).as_deref(), Some("jpg"));
        assert_eq!(normalize_ext(html).as_deref(), Some("html"));
        assert_eq!(normalize_ext(plain).as_deref(), Some("mp4"));
    }

    #[test]
    fn plan_selects_copy_vs_rename() {
        let src = Path::new("a.jpg");
        let dst = Path::new("b.jpeg");

        let plan_copy = build_plan(src, dst, false, ConversionOptions::default()).unwrap();
        assert_eq!(plan_copy.strategy, Strategy::CopyOnly);

        let plan_rename = build_plan(src, dst, true, ConversionOptions::default()).unwrap();
        assert_eq!(plan_rename.strategy, Strategy::RenameOnly);
    }

    #[test]
    fn plan_selects_convert() {
        let src = Path::new("a.png");
        let dst = Path::new("b.jpg");
        let plan = build_plan(src, dst, false, ConversionOptions::default()).unwrap();
        assert_eq!(plan.strategy, Strategy::Convert);
    }

    #[test]
    fn plan_selects_backend() {
        let image_plan = build_plan(
            Path::new("a.png"),
            Path::new("b.jpg"),
            false,
            ConversionOptions::default(),
        )
        .unwrap();
        assert_eq!(image_plan.backend, Some(Backend::ImageMagick));

        let media_plan = build_plan(
            Path::new("a.mp4"),
            Path::new("b.webm"),
            false,
            ConversionOptions::default(),
        )
        .unwrap();
        assert_eq!(media_plan.backend, Some(Backend::Ffmpeg));
    }

    #[test]
    fn rejects_invalid_quality() {
        let options = ConversionOptions {
            image_quality: Some(0),
            ..ConversionOptions::default()
        };
        let result = build_plan(Path::new("a.png"), Path::new("b.jpg"), false, options);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_preset() {
        let options = ConversionOptions {
            preset: Some("fastish".to_string()),
            ..ConversionOptions::default()
        };
        let result = build_plan(Path::new("a.mp4"), Path::new("b.webm"), false, options);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_bitrate() {
        let options = ConversionOptions {
            audio_bitrate: Some("12kbps".to_string()),
            ..ConversionOptions::default()
        };
        let result = build_plan(Path::new("a.wav"), Path::new("b.mp3"), false, options);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_codec() {
        let options = ConversionOptions {
            video_codec: Some("  ".to_string()),
            ..ConversionOptions::default()
        };
        let result = build_plan(Path::new("a.mp4"), Path::new("b.webm"), false, options);
        assert!(result.is_err());
    }
}
