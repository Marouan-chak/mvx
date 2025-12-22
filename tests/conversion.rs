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

fn tool_available_with_args(name: &str, args: &[&str]) -> bool {
    Command::new(name)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn imagemagick_pdf_support() -> (bool, bool) {
    let output = if tool_available("magick") {
        Command::new("magick").args(["-list", "format"]).output()
    } else if tool_available("convert") {
        Command::new("convert").args(["-list", "format"]).output()
    } else {
        return (false, false);
    };

    let output = match output {
        Ok(output) if output.status.success() => output.stdout,
        _ => return (false, false),
    };
    let text = String::from_utf8_lossy(&output);
    let mut read = false;
    let mut write = false;
    for line in text.lines() {
        let line = line.trim_start();
        if !line.starts_with("PDF") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let modes = parts[1];
        if modes.contains('r') {
            read = true;
        }
        if modes.contains('w') {
            write = true;
        }
        break;
    }
    (read, write)
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

#[test]
fn converts_document_with_libreoffice() {
    if !tool_available_with_args("soffice", &["--version"]) {
        eprintln!("skipping document conversion test; LibreOffice not available");
        return;
    }

    let temp_dir = TempDir::new().expect("temp dir");
    let input = temp_dir.path().join("input.txt");
    let output = temp_dir.path().join("output.pdf");

    std::fs::write(&input, "mvx test document").expect("write input");

    let status = Command::new(mvx_bin())
        .arg(&input)
        .arg(&output)
        .status()
        .expect("mvx failed to run");
    assert!(status.success(), "mvx document conversion failed");
    ensure_non_empty(&output);
}

#[test]
fn converts_image_to_pdf_with_imagemagick() {
    let (_pdf_read, pdf_write) = imagemagick_pdf_support();
    if !pdf_write {
        eprintln!("skipping image->pdf test; ImageMagick PDF write not available");
        return;
    }

    let temp_dir = TempDir::new().expect("temp dir");
    let input = temp_dir.path().join("input.png");
    let output = temp_dir.path().join("output.pdf");

    let create = if tool_available("magick") {
        let mut command = Command::new("magick");
        command.args(["-size", "16x16", "xc:skyblue"]).arg(&input);
        command
    } else {
        let mut command = Command::new("convert");
        command.args(["-size", "16x16", "xc:skyblue"]).arg(&input);
        command
    };
    assert!(run_status(create), "failed to create input image");

    let status = Command::new(mvx_bin())
        .arg(&input)
        .arg(&output)
        .status()
        .expect("mvx failed to run");
    assert!(status.success(), "mvx image->pdf conversion failed");
    ensure_non_empty(&output);
}

#[test]
fn converts_pdf_to_image_with_imagemagick() {
    let (pdf_read, pdf_write) = imagemagick_pdf_support();
    if !pdf_read || !pdf_write {
        eprintln!("skipping pdf->image test; ImageMagick PDF read/write not available");
        return;
    }

    let temp_dir = TempDir::new().expect("temp dir");
    let source_image = temp_dir.path().join("input.png");
    let source_pdf = temp_dir.path().join("input.pdf");
    let output = temp_dir.path().join("output.png");

    let create = if tool_available("magick") {
        let mut command = Command::new("magick");
        command
            .args(["-size", "16x16", "xc:orange"])
            .arg(&source_image);
        command
    } else {
        let mut command = Command::new("convert");
        command
            .args(["-size", "16x16", "xc:orange"])
            .arg(&source_image);
        command
    };
    assert!(run_status(create), "failed to create input image");

    let status = Command::new(mvx_bin())
        .arg(&source_image)
        .arg(&source_pdf)
        .status()
        .expect("mvx failed to run");
    if !status.success() {
        eprintln!("skipping pdf->image test; mvx could not create pdf");
        return;
    }

    let status = Command::new(mvx_bin())
        .arg(&source_pdf)
        .arg(&output)
        .status()
        .expect("mvx failed to run");
    assert!(status.success(), "mvx pdf->image conversion failed");
    ensure_non_empty(&output);
}
