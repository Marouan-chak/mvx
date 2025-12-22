use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub duration_seconds: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    format: Option<ProbeFormat>,
    streams: Option<Vec<ProbeStream>>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
}

pub fn probe_media(path: &Path) -> Result<MediaInfo> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_format")
        .arg("-show_streams")
        .arg("-print_format")
        .arg("json")
        .arg(path)
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("ffprobe not found; install ffmpeg (e.g., apt install ffmpeg)")
            } else {
                anyhow::Error::new(err).context("failed to execute ffprobe")
            }
        })?;

    if !output.status.success() {
        anyhow::bail!("ffprobe exited with status {}", output.status);
    }

    let parsed: ProbeOutput =
        serde_json::from_slice(&output.stdout).context("failed to parse ffprobe output")?;

    let duration_seconds = parsed
        .format
        .as_ref()
        .and_then(|fmt| fmt.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok());
    let mut video_codec = None;
    let mut audio_codec = None;
    if let Some(streams) = parsed.streams {
        for stream in streams {
            match stream.codec_type.as_deref() {
                Some("video") if video_codec.is_none() => {
                    video_codec = stream.codec_name;
                }
                Some("audio") if audio_codec.is_none() => {
                    audio_codec = stream.codec_name;
                }
                _ => {}
            }
        }
    }

    Ok(MediaInfo {
        duration_seconds,
        video_codec,
        audio_codec,
    })
}
