//! Daemon identity shared by CLI lifecycle and GUI status/action routing.
use super::cli::{Cli, Commands};
use super::processes::{self, Process};
use clap::Parser;
use std::path::Path;

mod status;
pub use status::StatusProbe;

fn matches(process: &Process, uid: u32) -> bool {
    if !process.owned_by(uid) || !super::daemon_is_running(process.pid) {
        return false;
    }
    let args = process.arguments.iter().filter(|arg| !arg.is_empty());
    // The real parser distinguishes option values named "start" from subcommands.
    Cli::try_parse_from(args).is_ok_and(|cli| matches!(cli.command, None | Some(Commands::Start)))
}

pub fn is_daemon_process(pid: u32, uid: u32) -> bool {
    Process::read(pid).is_some_and(|process| matches(&process, uid))
}

/// Prefer a validated PID-file entry, then recover other same-user daemons.
/// Diagnostic commands, zombies and this calling process never enter the result.
pub fn discover(pid_file: Option<&Path>) -> Vec<u32> {
    from_processes(
        &processes::same_user_processes().unwrap_or_default(),
        pid_file,
    )
}

/// Select daemon identities from one read-only process inventory.
pub fn from_processes(processes: &[Process], pid_file: Option<&Path>) -> Vec<u32> {
    let uid = unsafe { libc::getuid() };
    let tracked = pid_file
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.trim().parse::<u32>().ok());
    let mut pids = processes
        .iter()
        .filter(|process| process.pid != std::process::id() && matches(process, uid))
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    pids.sort_unstable_by_key(|&pid| (Some(pid) != tracked, pid));
    pids
}
