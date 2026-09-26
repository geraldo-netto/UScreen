//! Windows interactive daemon lifecycle; capture and input remain unsupported.
use anyhow::{bail, Result};
use blent_config::{
    cli::{Cli, Commands},
    windows::{lifecycle, runtime},
};
use clap::Parser;
use std::{path::Path, time::Duration};

pub(super) fn run() -> Result<()> {
    let cli = Cli::parse();
    blent_config::scheduling::apply_configured();
    let path = cli
        .runtime_dir
        .map(Ok)
        .unwrap_or_else(runtime::runtime_dir)?;
    match cli.command {
        None | Some(Commands::Start) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(serve(&path)),
        Some(Commands::Stop) => lifecycle::stop(&path, Duration::from_secs(5)),
        Some(Commands::Status) => status(&path),
        Some(Commands::Doctor) => {
            diagnostics();
            Ok(())
        }
        _ => bail!("Display, input, camera and connection backends are unsupported on Windows"),
    }
}
async fn serve(path: &Path) -> Result<()> {
    let session = lifecycle::Session::start(path)?;
    println!("Blent daemon running; display and input unsupported on Windows");
    loop {
        tokio::select! {
            result = tokio::signal::ctrl_c() => { result?; return Ok(()); },
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if session.stop_requested() { return Ok(()); }
            }
        }
    }
}
fn status(path: &Path) -> Result<()> {
    match lifecycle::status(path)? {
        Some(owner) => println!(
            "Blent daemon running (PID {}); display and input unsupported",
            owner.pid
        ),
        None => println!("Blent daemon stopped; display and input unsupported"),
    }
    Ok(())
}
fn diagnostics() {
    println!("Blent Windows build preview");
    match blent_config::config_path() {
        Ok(path) => println!("Configuration: {}", path.display()),
        Err(error) => println!("Configuration: unavailable ({error})"),
    }
    println!("Blent version: {}", env!("CARGO_PKG_VERSION"));
    for line in blent_config::diagnostics::collect().lines() {
        println!("{line}");
    }
}
