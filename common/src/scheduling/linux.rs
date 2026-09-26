//! Linux CPU groups cover all threads and inherit across fork/exec. Never
//! raise an enclosing editor/session group or request real-time scheduling.
use super::Priority;
use crate::{commands::SyncCommandExt, linux::processes::Process};
use anyhow::{ensure, Context, Result};
use std::{path::Path, process::Command, time::Duration};

fn weight(priority: Priority) -> u32 {
    match priority {
        Priority::Normal => 100,
        Priority::High => 1000,
    }
}

fn group_path(text: &str) -> Result<&str> {
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .context("unified CPU controller unavailable")?;
    ensure!(
        path.starts_with('/') && !path.split('/').any(|part| part == ".."),
        "invalid CPU group path"
    );
    Ok(path)
}

fn owned_unit(path: &str, pid: u32) -> Option<&str> {
    let unit = path.rsplit('/').next()?;
    let private = format!("blent-priority-{pid}.scope");
    (unit == "blent.service" || unit == private).then_some(unit)
}

fn scope_arguments(pid: u32, weight: u32) -> Vec<String> {
    [
        "--user",
        "--timeout=2s",
        "call",
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "StartTransientUnit",
        "ssa(sv)a(sa(sv))",
        &format!("blent-priority-{pid}.scope"),
        "fail",
        "3",
        "PIDs",
        "au",
        "1",
        &pid.to_string(),
        "Slice",
        "s",
        "app.slice",
        "CPUWeight",
        "t",
        &weight.to_string(),
        "0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn run(program: &str, args: &[String]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .output_timeout(Duration::from_secs(3))?;
    ensure!(
        output.status.success(),
        "{program}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn request(
    pid: u32,
    priority: Priority,
    text: &str,
    run: impl FnOnce(&str, &[String]) -> Result<()>,
) -> Result<()> {
    let weight = weight(priority);
    if let Some(unit) = owned_unit(group_path(text)?, pid) {
        run(
            "systemctl",
            &[
                "--user".into(),
                "set-property".into(),
                "--runtime".into(),
                unit.into(),
                format!("CPUWeight={weight}"),
            ],
        )
    } else {
        run("busctl", &scope_arguments(pid, weight))
    }
}

fn verify(root: &Path, text: &str, priority: Priority) -> Result<String> {
    let path = group_path(text)?;
    let effective =
        std::fs::read_to_string(root.join(path.trim_start_matches('/')).join("cpu.weight"))?;
    ensure!(
        effective.trim().parse::<u32>()? == weight(priority),
        "CPU weight request not effective"
    );
    Ok(format!(
        "CPUWeight={} ({path}); all threads and new children",
        weight(priority)
    ))
}

fn apply_pid(pid: u32, priority: Priority) -> Result<String> {
    let path = format!("/proc/{pid}/cgroup");
    request(pid, priority, &std::fs::read_to_string(&path)?, run)?;
    wait_effective(Duration::from_secs(2), || {
        let text = std::fs::read_to_string(&path)?;
        ensure!(
            owned_unit(group_path(&text)?, pid).is_some(),
            "priority scope is still starting"
        );
        verify(Path::new("/sys/fs/cgroup"), &text, priority)
    })
}

fn wait_effective(timeout: Duration, mut read: impl FnMut() -> Result<String>) -> Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match read() {
            Ok(value) => return Ok(value),
            Err(error) if std::time::Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

pub(super) fn apply_current(priority: Priority) -> Result<String> {
    apply_pid(std::process::id(), priority)
}

fn shared_server(process: &Process, executable: &Path, uid: u32) -> bool {
    process.owned_by(uid)
        && process.executable == executable
        && process
            .arguments
            .windows(2)
            .any(|pair| pair[0] == "fork-server" && pair[1] == "server")
        && process
            .arguments
            .windows(2)
            .any(|pair| pair[0] == "-L" && pair[1] == "tcp:5037")
}

pub(super) fn apply_shared_adb(priority: Priority) -> Result<()> {
    let executables = adb_executables(&std::env::var_os("PATH").unwrap_or_default());
    if executables.is_empty() {
        return Ok(());
    }
    for entry in std::fs::read_dir("/proc")? {
        let name = entry?.file_name();
        let Some(process) = name
            .to_str()
            .and_then(|name| name.parse().ok())
            .and_then(Process::read)
        else {
            continue;
        };
        if executables.contains(&process.executable) {
            apply_server(&process, &process.executable, priority)?;
        }
    }
    Ok(())
}

fn adb_executables(
    search_path: &std::ffi::OsStr,
) -> std::collections::BTreeSet<std::path::PathBuf> {
    std::env::split_paths(search_path)
        .map(|directory| directory.join("adb"))
        .filter(|path| crate::linux::programs::is_executable(path))
        .filter_map(|path| path.canonicalize().ok())
        .collect()
}

fn apply_server(process: &Process, executable: &Path, priority: Priority) -> Result<()> {
    if !shared_server(process, executable, unsafe { libc::geteuid() }) {
        return Ok(());
    }
    ensure!(
        Process::read(process.pid).as_ref() == Some(process),
        "ADB identity changed"
    );
    let effective = apply_pid(process.pid, priority)?;
    tracing::info!("Shared ADB scheduling: {effective}");
    Ok(())
}

#[cfg(test)]
mod tests;
