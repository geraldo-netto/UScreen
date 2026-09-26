//! T494 compilation milestone: no capture/input/lifecycle backend is enabled.
use anyhow::{bail, Result};
use blent_config::cli::{Cli, Commands};
use clap::Parser;

pub(super) fn run() -> Result<()> {
    let cli = Cli::parse();
    blent_config::scheduling::apply_configured();
    if matches!(cli.command, Some(Commands::Doctor)) {
        diagnostics();
    }
    bail!("Daemon, display and input backends are not implemented on Windows; this build provides configuration diagnostics only")
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
