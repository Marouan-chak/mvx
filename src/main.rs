mod config;
mod detect;
mod execute;
mod ffprobe;
mod plan;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "mvx",
    version,
    about = "Move or convert files based on destination extension"
)]
struct Cli {
    /// Source file path
    source: PathBuf,
    /// Destination file path
    destination: PathBuf,
    /// Show the plan without executing
    #[arg(long)]
    plan: bool,
    /// Alias for --plan
    #[arg(long)]
    dry_run: bool,
    /// Overwrite destination if it exists
    #[arg(long)]
    overwrite: bool,
    /// Backup destination if it exists (adds .bak, .bak.1, ...)
    #[arg(long)]
    backup: bool,
    /// Path to config file (defaults to XDG config path)
    #[arg(long)]
    config: Option<PathBuf>,
    /// Config profile name
    #[arg(long)]
    profile: Option<String>,
    /// Move (delete source) instead of keeping the source
    #[arg(long)]
    move_source: bool,
    /// Image quality (1-100) for ImageMagick conversions
    #[arg(long)]
    image_quality: Option<u8>,
    /// Video bitrate (e.g. 2500k) for ffmpeg conversions
    #[arg(long)]
    video_bitrate: Option<String>,
    /// Audio bitrate (e.g. 192k) for ffmpeg conversions
    #[arg(long)]
    audio_bitrate: Option<String>,
    /// Encoder preset (e.g. ultrafast, fast, medium) for ffmpeg conversions
    #[arg(long)]
    preset: Option<String>,
    /// ffmpeg video codec (e.g. libx264, libx265, vp9)
    #[arg(long)]
    video_codec: Option<String>,
    /// ffmpeg audio codec (e.g. aac, libopus, flac)
    #[arg(long)]
    audio_codec: Option<String>,
    /// Force ffmpeg stream copy (no re-encode) when possible
    #[arg(long)]
    stream_copy: bool,
    /// Force ffmpeg transcode (re-encode)
    #[arg(long)]
    transcode: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.stream_copy && cli.transcode {
        anyhow::bail!("--stream-copy and --transcode are mutually exclusive");
    }
    if cli.overwrite && cli.backup {
        anyhow::bail!("--overwrite and --backup are mutually exclusive");
    }
    let mut options = plan::ConversionOptions::default();
    if let Some(config_options) =
        config::load_options(cli.config.as_deref(), cli.profile.as_deref())?
    {
        options = config_options;
    }

    if let Some(value) = cli.image_quality {
        options.image_quality = Some(value);
    }
    if let Some(value) = cli.video_bitrate {
        options.video_bitrate = Some(value);
    }
    if let Some(value) = cli.audio_bitrate {
        options.audio_bitrate = Some(value);
    }
    if let Some(value) = cli.preset {
        options.preset = Some(value);
    }
    if let Some(value) = cli.video_codec {
        options.video_codec = Some(value);
    }
    if let Some(value) = cli.audio_codec {
        options.audio_codec = Some(value);
    }
    options.ffmpeg_preference = if cli.stream_copy {
        plan::FfmpegPreference::StreamCopy
    } else if cli.transcode {
        plan::FfmpegPreference::Transcode
    } else {
        options.ffmpeg_preference
    };

    let plan = plan::build_plan(
        &cli.source,
        &cli.destination,
        cli.move_source,
        cli.backup,
        options,
    )
    .context("failed to build plan")?;

    if cli.plan || cli.dry_run {
        println!("{}", plan::render_plan(&plan, cli.overwrite));
        return Ok(());
    }

    execute::execute_plan(&plan, cli.overwrite).context("execution failed")?;
    Ok(())
}
