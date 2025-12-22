use crate::ffprobe::probe_media;
use crate::plan::{Backend, FfmpegMode, MediaKind, Plan, Strategy};
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::Builder;

pub fn execute_plan(plan: &Plan, overwrite: bool) -> Result<()> {
    ensure_parent_dir(&plan.destination)?;
    if plan.destination.exists() && !overwrite {
        bail!("destination exists; pass --overwrite to replace it");
    }

    match plan.strategy {
        Strategy::RenameOnly => rename_only(&plan.source, &plan.destination, overwrite),
        Strategy::CopyOnly => copy_only(&plan.source, &plan.destination, overwrite),
        Strategy::Convert => convert(plan, overwrite),
    }
}

fn rename_only(source: &Path, destination: &Path, overwrite: bool) -> Result<()> {
    if overwrite && destination.exists() {
        fs::remove_file(destination).context("failed to remove existing destination")?;
    }
    fs::rename(source, destination).context("failed to rename source")
}

fn copy_only(source: &Path, destination: &Path, overwrite: bool) -> Result<()> {
    if overwrite && destination.exists() {
        fs::remove_file(destination).context("failed to remove existing destination")?;
    }

    let parent = destination
        .parent()
        .context("destination must have a parent directory")?;
    let mut temp = Builder::new()
        .prefix(".mvx.tmp")
        .tempfile_in(parent)
        .context("failed to create temp file")?;
    let mut input = fs::File::open(source).context("failed to open source")?;
    io::copy(&mut input, &mut temp).context("failed to copy data")?;
    temp.persist(destination)
        .context("failed to finalize destination")?;
    Ok(())
}

fn convert(plan: &Plan, overwrite: bool) -> Result<()> {
    let backend = plan
        .backend
        .context("no backend available for conversion")?;
    let parent = plan
        .destination
        .parent()
        .context("destination must have a parent directory")?;
    let temp_dir = Builder::new()
        .prefix(".mvx.tmp")
        .tempdir_in(parent)
        .context("failed to create temp directory")?;
    let temp_path = temp_output_path(temp_dir.path(), &plan.destination);

    match backend {
        Backend::ImageMagick => run_imagemagick(&plan.source, &temp_path, &plan.options)?,
        Backend::Ffmpeg => {
            let info = probe_media(&plan.source).ok();
            let mode = decide_ffmpeg_mode(plan, info.as_ref());
            run_ffmpeg(
                &plan.source,
                &temp_path,
                &plan.options,
                plan.dest_kind,
                mode,
                info.as_ref().and_then(|i| i.duration_seconds),
            )?;
        }
    }

    ensure_non_empty(&temp_path)?;
    finalize_output(&temp_path, &plan.destination, overwrite)?;

    if plan.move_source {
        fs::remove_file(&plan.source).context("failed to remove source")?;
    }

    Ok(())
}

fn run_imagemagick(
    source: &Path,
    dest: &Path,
    options: &crate::plan::ConversionOptions,
) -> Result<()> {
    let mut command = Command::new("magick");
    command.arg(source);
    if let Some(quality) = options.image_quality {
        command.arg("-quality").arg(quality.to_string());
    }
    command.arg(dest);
    let status = command
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status();

    let status = match status {
        Ok(status) => status,
        Err(_) => {
            let mut command = Command::new("convert");
            command.arg(source);
            if let Some(quality) = options.image_quality {
                command.arg("-quality").arg(quality.to_string());
            }
            command.arg(dest);
            let status = command
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .status()
                .context("failed to execute ImageMagick convert")?;
            return handle_status(status, "ImageMagick");
        }
    };

    handle_status(status, "ImageMagick")
}

fn run_ffmpeg(
    source: &Path,
    dest: &Path,
    options: &crate::plan::ConversionOptions,
    dest_kind: MediaKind,
    mode: FfmpegMode,
    duration_seconds: Option<f64>,
) -> Result<()> {
    let mut command = Command::new("ffmpeg");
    command
        .arg("-nostdin")
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(source);
    if mode == FfmpegMode::StreamCopy {
        command.arg("-c").arg("copy");
    } else if dest_kind == MediaKind::Video {
        if let Some(codec) = options.video_codec.as_deref() {
            command.arg("-c:v").arg(codec);
        }
        if let Some(bitrate) = options.video_bitrate.as_deref() {
            command.arg("-b:v").arg(bitrate);
        }
        if let Some(preset) = options.preset.as_deref() {
            command.arg("-preset").arg(preset);
        }
        if let Some(codec) = options.audio_codec.as_deref() {
            command.arg("-c:a").arg(codec);
        }
        if let Some(bitrate) = options.audio_bitrate.as_deref() {
            command.arg("-b:a").arg(bitrate);
        }
    } else if dest_kind == MediaKind::Audio {
        if let Some(codec) = options.audio_codec.as_deref() {
            command.arg("-c:a").arg(codec);
        }
        if let Some(bitrate) = options.audio_bitrate.as_deref() {
            command.arg("-b:a").arg(bitrate);
        }
    }
    command.arg("-progress").arg("pipe:1");
    let mut child = command
        .arg(dest)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to execute ffmpeg")?;

    if let Some(stdout) = child.stdout.take() {
        stream_progress(stdout, duration_seconds);
    }

    let status = child.wait().context("failed to wait for ffmpeg")?;

    handle_status(status, "ffmpeg")
}

fn decide_ffmpeg_mode(plan: &Plan, info: Option<&crate::ffprobe::MediaInfo>) -> FfmpegMode {
    match plan.options.ffmpeg_preference {
        crate::plan::FfmpegPreference::StreamCopy => return FfmpegMode::StreamCopy,
        crate::plan::FfmpegPreference::Transcode => return FfmpegMode::Transcode,
        crate::plan::FfmpegPreference::Auto => {}
    }
    if plan.dest_kind == MediaKind::Audio {
        return FfmpegMode::Transcode;
    }
    let dest_ext = match plan.dest_ext.as_deref() {
        Some(ext) => ext,
        None => return FfmpegMode::Transcode,
    };
    let Some(info) = info else {
        return FfmpegMode::Transcode;
    };
    let Some(video) = info.video_codec.as_deref() else {
        return FfmpegMode::Transcode;
    };
    let audio = info.audio_codec.as_deref();

    if dest_ext == "mkv" {
        return FfmpegMode::StreamCopy;
    }

    match dest_ext {
        "mp4" | "mov" => {
            let video_ok = matches!(video, "h264" | "hevc" | "mpeg4" | "av1");
            let audio_ok = audio
                .map(|codec| matches!(codec, "aac" | "mp3" | "alac"))
                .unwrap_or(true);
            if video_ok && audio_ok {
                FfmpegMode::StreamCopy
            } else {
                FfmpegMode::Transcode
            }
        }
        "webm" => {
            let video_ok = matches!(video, "vp8" | "vp9" | "av1");
            let audio_ok = audio
                .map(|codec| matches!(codec, "opus" | "vorbis"))
                .unwrap_or(true);
            if video_ok && audio_ok {
                FfmpegMode::StreamCopy
            } else {
                FfmpegMode::Transcode
            }
        }
        _ => FfmpegMode::Transcode,
    }
}

fn stream_progress(stdout: impl std::io::Read, duration_seconds: Option<f64>) {
    let reader = BufReader::new(stdout);
    let mut last_percent: Option<f64> = None;
    for line in reader.lines().map_while(Result::ok) {
        if line == "progress=end" {
            if duration_seconds.is_some() && last_percent.is_none_or(|percent| percent < 99.5) {
                println!("Progress: 100%");
            }
            continue;
        }
        let Some(value) = line.strip_prefix("out_time_ms=") else {
            continue;
        };
        let Ok(ms) = value.trim().parse::<u64>() else {
            continue;
        };
        let Some(duration) = duration_seconds else {
            continue;
        };
        let elapsed = ms as f64 / 1_000_000.0;
        if duration <= 0.0 {
            continue;
        }
        let percent = ((elapsed / duration) * 100.0).min(100.0);
        if last_percent.is_none_or(|last| (percent - last).abs() >= 1.0) {
            let remaining = (duration - elapsed).max(0.0);
            println!("Progress: {:.0}% eta {:.1}s", percent, remaining);
            last_percent = Some(percent);
        }
    }
}

fn handle_status(status: std::process::ExitStatus, name: &str) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        bail!("{name} exited with status {status}")
    }
}

fn temp_output_path(temp_dir: &Path, destination: &Path) -> PathBuf {
    let suffix = destination
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext))
        .unwrap_or_else(|| ".out".to_string());
    temp_dir.join(format!("output{}", suffix))
}

fn ensure_non_empty(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path).context("failed to stat output")?;
    if metadata.len() == 0 {
        bail!("output file is empty");
    }
    Ok(())
}

fn finalize_output(temp_path: &Path, destination: &Path, overwrite: bool) -> Result<()> {
    if overwrite && destination.exists() {
        fs::remove_file(destination).context("failed to remove existing destination")?;
    }
    fs::rename(temp_path, destination).context("failed to finalize destination")?;
    Ok(())
}

fn ensure_parent_dir(destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("destination must have a parent directory")?;
    fs::create_dir_all(parent).context("failed to create destination directory")?;
    Ok(())
}
