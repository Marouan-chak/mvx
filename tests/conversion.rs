use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn mvx_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mvx"))
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_status(mut command: Command) -> bool {
    command.status().map(|s| s.success()).unwrap_or(false)
}

fn ensure_non_empty(path: &Path) {
    let metadata = std::fs::metadata(path).expect("missing output file");
    assert!(metadata.len() > 0, "output file is empty");
}

#[test]
fn converts_image_with_imagemagick() {
    let has_magick = tool_available("magick");
    let has_convert = tool_available("convert");
    if !has_magick && !has_convert {
        eprintln!("skipping image conversion test; ImageMagick not available");
        return;
    }

    let temp_dir = TempDir::new().expect("temp dir");
    let input = temp_dir.path().join("input.png");
    let output = temp_dir.path().join("output.jpg");

    let create = if has_magick {
        let mut command = Command::new("magick");
        command.args(["-size", "1x1", "xc:red"]).arg(&input);
        command
    } else {
        let mut command = Command::new("convert");
        command.args(["-size", "1x1", "xc:red"]).arg(&input);
        command
    };
    assert!(run_status(create), "failed to create input image");

    let status = Command::new(mvx_bin())
        .arg(&input)
        .arg(&output)
        .status()
        .expect("mvx failed to run");
    assert!(status.success(), "mvx conversion failed");
    assert!(input.exists(), "source should be kept by default");
    ensure_non_empty(&output);
}

#[test]
fn converts_audio_with_ffmpeg() {
    if !tool_available("ffmpeg") {
        eprintln!("skipping audio conversion test; ffmpeg not available");
        return;
    }

    let temp_dir = TempDir::new().expect("temp dir");
    let input = temp_dir.path().join("input.wav");
    let output = temp_dir.path().join("output.flac");

    let create_status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:duration=0.2",
        ])
        .arg(&input)
        .status()
        .expect("ffmpeg failed to run");
    if !create_status.success() {
        eprintln!("skipping audio conversion test; ffmpeg cannot create wav");
        return;
    }

    let status = Command::new(mvx_bin())
        .arg(&input)
        .arg(&output)
        .arg("--transcode")
        .status()
        .expect("mvx failed to run");
    assert!(status.success(), "mvx conversion failed");
    ensure_non_empty(&output);
}

#[test]
fn stream_copy_forced_audio_fails() {
    if !tool_available("ffmpeg") {
        eprintln!("skipping stream copy test; ffmpeg not available");
        return;
    }

    let temp_dir = TempDir::new().expect("temp dir");
    let input = temp_dir.path().join("input.wav");
    let output = temp_dir.path().join("output.flac");

    let create_status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:duration=0.2",
        ])
        .arg(&input)
        .status()
        .expect("ffmpeg failed to run");
    if !create_status.success() {
        eprintln!("skipping stream copy test; ffmpeg cannot create wav");
        return;
    }

    let status = Command::new(mvx_bin())
        .arg(&input)
        .arg(&output)
        .arg("--stream-copy")
        .status()
        .expect("mvx failed to run");
    assert!(
        !status.success(),
        "stream-copy should fail for audio conversion"
    );
}

#[test]
fn stream_copy_forced_video_succeeds() {
    if !tool_available("ffmpeg") {
        eprintln!("skipping video stream copy test; ffmpeg not available");
        return;
    }

    let temp_dir = TempDir::new().expect("temp dir");
    let input = temp_dir.path().join("input.mp4");
    let output = temp_dir.path().join("output.mov");

    let create_status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=32x32:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:duration=0.2",
            "-shortest",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "64k",
        ])
        .arg(&input)
        .status()
        .expect("ffmpeg failed to run");
    if !create_status.success() {
        eprintln!("skipping video stream copy test; ffmpeg cannot create mp4");
        return;
    }

    let status = Command::new(mvx_bin())
        .arg(&input)
        .arg(&output)
        .arg("--stream-copy")
        .status()
        .expect("mvx failed to run");
    assert!(status.success(), "mvx stream-copy conversion failed");
    ensure_non_empty(&output);
}

#[test]
fn transcode_with_codec_flags() {
    if !tool_available("ffmpeg") {
        eprintln!("skipping codec flag test; ffmpeg not available");
        return;
    }

    let temp_dir = TempDir::new().expect("temp dir");
    let input = temp_dir.path().join("input.mkv");
    let output = temp_dir.path().join("output.mp4");

    let create_status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=32x32:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:duration=0.2",
            "-shortest",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "64k",
        ])
        .arg(&input)
        .status()
        .expect("ffmpeg failed to run");
    if !create_status.success() {
        eprintln!("skipping codec flag test; ffmpeg cannot create mkv");
        return;
    }

    let status = Command::new(mvx_bin())
        .arg(&input)
        .arg(&output)
        .args([
            "--transcode",
            "--video-codec",
            "libx264",
            "--audio-codec",
            "aac",
        ])
        .status()
        .expect("mvx failed to run");
    assert!(status.success(), "mvx codec flag conversion failed");
    ensure_non_empty(&output);
}
