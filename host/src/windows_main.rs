//! T494 compilation milestone: no capture/input/lifecycle backend is enabled.
use anyhow::{bail, Result};
use clap::Parser;
use uscreen_config::{
    cli::{Cli, Commands},
    platform,
};

pub(super) fn run() -> Result<()> {
    let cli = Cli::parse();
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
    let adb = if platform::programs::command_exists("adb") {
        "found"
    } else {
        "not found"
    };
    println!("ADB: {adb} (discovery does not establish a device connection)");
    println!("Display: unavailable\nInput: unavailable\nDaemon lifecycle: unavailable");
}
