//! T494 compilation milestone: no capture/input/lifecycle backend is enabled.
use anyhow::{bail, Result};
use clap::Parser;
use uscreen_config::cli::{Cli, Commands};

pub(super) fn run() -> Result<()> {
    let cli = Cli::parse();
    uscreen_config::scheduling::apply_configured();
    if matches!(cli.command, Some(Commands::Doctor)) {
        diagnostics();
    }
    bail!("Daemon, display and input backends are not implemented on Windows; this build provides configuration diagnostics only")
}

fn diagnostics() {
    println!("UScreen Windows build preview");
    match uscreen_config::config_path() {
        Ok(path) => println!("Configuration: {}", path.display()),
        Err(error) => println!("Configuration: unavailable ({error})"),
    }
    println!("UScreen version: {}", env!("CARGO_PKG_VERSION"));
    for line in uscreen_config::diagnostics::collect().lines() {
        println!("{line}");
    }
}
