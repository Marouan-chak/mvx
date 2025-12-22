use crate::plan::{ConversionOptions, FfmpegPreference};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    default: Profile,
    #[serde(default)]
    profile: HashMap<String, Profile>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct Profile {
    image_quality: Option<u8>,
    video_bitrate: Option<String>,
    audio_bitrate: Option<String>,
    preset: Option<String>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    ffmpeg_preference: Option<String>,
}

pub fn load_options(
    path: Option<&Path>,
    profile: Option<&str>,
) -> Result<Option<ConversionOptions>> {
    let config_path = match path {
        Some(path) => path.to_path_buf(),
        None => default_config_path()?,
    };

    if !config_path.exists() {
        return if path.is_some() {
            anyhow::bail!("config file not found: {}", config_path.display())
        } else {
            Ok(None)
        };
    }

    let contents = fs::read_to_string(&config_path)
        .with_context(|| format!("read {}", config_path.display()))?;
    let parsed: ConfigFile =
        toml::from_str(&contents).with_context(|| format!("parse {}", config_path.display()))?;

    let mut options = ConversionOptions::default();
    apply_profile(&parsed.default, &mut options)?;

    if let Some(name) = profile {
        if let Some(profile) = parsed.profile.get(name) {
            apply_profile(profile, &mut options)?;
        } else {
            anyhow::bail!("profile not found in config: {}", name);
        }
    }

    Ok(Some(options))
}

fn apply_profile(profile: &Profile, options: &mut ConversionOptions) -> Result<()> {
    if let Some(value) = profile.image_quality {
        options.image_quality = Some(value);
    }
    if let Some(value) = profile.video_bitrate.as_deref() {
        options.video_bitrate = Some(value.to_string());
    }
    if let Some(value) = profile.audio_bitrate.as_deref() {
        options.audio_bitrate = Some(value.to_string());
    }
    if let Some(value) = profile.preset.as_deref() {
        options.preset = Some(value.to_string());
    }
    if let Some(value) = profile.video_codec.as_deref() {
        options.video_codec = Some(value.to_string());
    }
    if let Some(value) = profile.audio_codec.as_deref() {
        options.audio_codec = Some(value.to_string());
    }
    if let Some(value) = profile.ffmpeg_preference.as_deref() {
        options.ffmpeg_preference = parse_preference(value)?;
    }
    Ok(())
}

fn parse_preference(value: &str) -> Result<FfmpegPreference> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(FfmpegPreference::Auto),
        "stream-copy" | "stream_copy" => Ok(FfmpegPreference::StreamCopy),
        "transcode" => Ok(FfmpegPreference::Transcode),
        _ => anyhow::bail!("invalid ffmpeg_preference: {}", value),
    }
}

fn default_config_path() -> Result<PathBuf> {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            let home = std::env::var("HOME").context("HOME not set")?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("mvx").join("config.toml"))
}
