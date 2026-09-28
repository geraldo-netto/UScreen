//! Development tooling; never starts capture or enables live candidates.
use anyhow::{Context, Result};
use blent::encoder_probe::{Key, Programs};
use std::{ffi::OsString, io::Read};

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .context("usage: probe_encoders <decoder-capabilities-v2.json> [encoder]")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 65_536, "Decoder advertisement too large");
    let key = Key {
        epoch: 1,
        decoders: serde_json::from_slice(&bytes)?,
        encoder: std::env::args().nth(2),
        bitrate: 20000,
        quality: 18,
        workers: 0,
        ten_bit: false,
    };
    let programs = Programs {
        ffmpeg: OsString::from(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        }),
        ffprobe: OsString::from(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        }),
    };
    let selection = programs.select(key.clone()).await?;
    for attempt in &selection.attempts {
        println!("{attempt:#?}");
    }
    println!(
        "Selected probe candidate: {:?}",
        selection.best(&key).map(|a| &a.encoder)
    );
    println!("Tooling only: capture/session/GPU acceptance is separate.");
    Ok(())
}
