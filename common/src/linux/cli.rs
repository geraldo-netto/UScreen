//! One CLI grammar for invocation and same-user daemon identification.
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "uscreen",
    version,
    about = "USB second-screen server for Linux"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Explicit EDID override. By default an EDID is generated at runtime
    /// for the configured (or tablet-reported) resolution.
    #[arg(long = "edid")]
    pub edid: Option<PathBuf>,

    #[arg(long = "helper")]
    pub helper: Option<PathBuf>,

    /// Defaults come from ~/.config/uscreen/config.toml; CLI flags override.
    #[arg(long = "encoder")]
    pub encoder: Option<String>,

    #[arg(long = "fps")]
    pub fps: Option<u32>,

    #[arg(long = "bitrate")]
    pub bitrate: Option<u32>,

    #[arg(long = "width")]
    pub width: Option<u32>,

    #[arg(long = "height")]
    pub height: Option<u32>,

    #[arg(long = "quality")]
    pub quality: Option<u32>,

    /// Integer downscale for the stream only; the desktop keeps its native mode.
    #[arg(long = "stream-scale")]
    pub stream_scale: Option<u32>,

    /// Linux conversion participants per helper: auto (default), or 1–128.
    #[arg(long = "conversion-threads", value_parser = parse_conversion_threads)]
    pub conversion_threads: Option<u32>,

    /// Drive the laptop's own screen with the pen instead of streaming a second
    /// display to the tablet.
    #[arg(long = "pen-only")]
    pub pen_only: bool,

    #[arg(long = "video-port")]
    pub video_port: Option<u16>,

    #[arg(long = "input-port")]
    pub input_port: Option<u16>,
}

fn parse_conversion_threads(value: &str) -> Result<u32, String> {
    if value == "auto" {
        return Ok(0);
    }
    let count: u32 = value.parse().map_err(|_| "Use auto or 1–128".to_string())?;
    if count > crate::model::MAX_CONVERSION_THREADS {
        return Err("Use auto or 1–128".into());
    }
    Ok(count)
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the uscreen daemon
    Start,
    /// Stop the uscreen daemon
    Stop,
    /// Show daemon status
    Status,
    /// List available displays
    ListDisplays,
    /// Set the tablet up to connect over Wi-Fi, so the cable becomes optional
    Wifi {
        /// Forget the remembered address and stop reconnecting
        #[arg(long = "off")]
        off: bool,
    },
    /// Diagnose the whole setup and report what is wrong
    Doctor,
}
