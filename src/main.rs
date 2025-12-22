mod batch;
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
    /// Source file path (single mode)
    source: Option<PathBuf>,
    /// Destination file path (single mode)
    destination: Option<PathBuf>,
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
    /// Enable batch mode
    #[arg(long)]
    batch: bool,
    /// Destination directory for batch mode
    #[arg(long, requires = "batch")]
    dest_dir: Option<PathBuf>,
    /// Additional inputs for batch mode (repeatable)
    #[arg(long)]
    input: Vec<String>,
    /// Read inputs from stdin (newline-separated)
    #[arg(long)]
    stdin: bool,
    /// Recurse into directories for batch mode
    #[arg(long)]
    recursive: bool,
    /// Change destination extension for batch mode (e.g., mp3)
    #[arg(long)]
    to_ext: Option<String>,
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
    /// Emit JSON output
    #[arg(long)]
    json: bool,
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
    if let Some(value) = cli.video_bitrate.as_deref() {
        options.video_bitrate = Some(value.to_string());
    }
    if let Some(value) = cli.audio_bitrate.as_deref() {
        options.audio_bitrate = Some(value.to_string());
    }
    if let Some(value) = cli.preset.as_deref() {
        options.preset = Some(value.to_string());
    }
    if let Some(value) = cli.video_codec.as_deref() {
        options.video_codec = Some(value.to_string());
    }
    if let Some(value) = cli.audio_codec.as_deref() {
        options.audio_codec = Some(value.to_string());
    }
    options.ffmpeg_preference = if cli.stream_copy {
        plan::FfmpegPreference::StreamCopy
    } else if cli.transcode {
        plan::FfmpegPreference::Transcode
    } else {
        options.ffmpeg_preference
    };

    if cli.batch {
        run_batch(&cli, options)?;
        return Ok(());
    }

    let source = cli.source.context("source is required")?;
    let destination = cli.destination.context("destination is required")?;
    let plan = plan::build_plan(&source, &destination, cli.move_source, cli.backup, options)
        .context("failed to build plan")?;

    if cli.plan || cli.dry_run {
        if cli.json {
            println!("{}", plan::render_plan_json(&plan, cli.overwrite)?);
        } else {
            println!("{}", plan::render_plan(&plan, cli.overwrite));
        }
        return Ok(());
    }

    execute::execute_plan(&plan, cli.overwrite).context("execution failed")?;
    if cli.json {
        let output = serde_json::json!({
            "status": "ok",
            "source": plan.source.display().to_string(),
            "destination": plan.destination.display().to_string()
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }
    Ok(())
}

fn run_batch(cli: &Cli, options: plan::ConversionOptions) -> Result<()> {
    let dest_dir = cli
        .dest_dir
        .as_ref()
        .context("batch mode requires --dest-dir")?;

    let mut inputs = Vec::new();
    if let Some(source) = cli.source.as_ref() {
        inputs.push(source.to_string_lossy().to_string());
    }
    inputs.extend(cli.input.iter().cloned());

    let stdin_sources = if cli.stdin {
        read_stdin_lines()?
    } else {
        Vec::new()
    };

    let sources = batch::collect_sources(&inputs, stdin_sources, cli.recursive)?;
    if sources.is_empty() {
        anyhow::bail!("no inputs provided for batch mode");
    }

    let batch_input = batch::BatchInput {
        dest_dir: dest_dir.clone(),
        to_ext: cli.to_ext.clone(),
    };

    let mut ok = 0usize;
    let mut failed = Vec::new();

    for source in sources {
        let destination = match batch::dest_for_source(&batch_input, &source) {
            Ok(dest) => dest,
            Err(err) => {
                failed.push((source, err));
                continue;
            }
        };
        let plan = match plan::build_plan(
            &source,
            &destination,
            cli.move_source,
            cli.backup,
            options.clone(),
        ) {
            Ok(plan) => plan,
            Err(err) => {
                failed.push((source, err));
                continue;
            }
        };
        if cli.plan || cli.dry_run {
            if cli.json {
                println!("{}", plan::render_plan_json(&plan, cli.overwrite)?);
            } else {
                println!("---");
                println!("{}", plan::render_plan(&plan, cli.overwrite));
            }
            ok += 1;
            continue;
        }
        match execute::execute_plan(&plan, cli.overwrite) {
            Ok(_) => ok += 1,
            Err(err) => failed.push((source, err)),
        }
    }

    let total = ok + failed.len();
    if cli.json {
        let output = serde_json::json!({
            "status": if failed.is_empty() { "ok" } else { "failed" },
            "total": total,
            "succeeded": ok,
            "failed": failed.len(),
            "failures": failed.iter().map(|(source, err)| {
                serde_json::json!({
                    "source": source.display().to_string(),
                    "error": err.to_string()
                })
            }).collect::<Vec<_>>()
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Batch summary: total {total}, succeeded {ok}, failed {}",
            failed.len()
        );
    }
    if !failed.is_empty() {
        if !cli.json {
            for (source, err) in failed {
                println!("Fail: {} -> {}", source.display(), err);
            }
        }
        anyhow::bail!("batch completed with failures");
    }
    Ok(())
}

fn read_stdin_lines() -> Result<Vec<String>> {
    use std::io::Read;
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("read stdin")?;
    Ok(input
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}
