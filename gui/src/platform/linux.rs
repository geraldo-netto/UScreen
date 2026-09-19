//! Linux lifecycle, session status and privileged setup adapters.
use super::find_uscreen_bin;
use crate::Status;
use std::{path::PathBuf, process::Command, time::Duration};
use uscreen_config::{
    commands::{daemon_command_timeout, SyncCommandExt},
    linux::daemon,
};

/// Autostart can be a user service or an XDG desktop entry.
pub(crate) fn autostart_enabled() -> bool {
    uscreen_config::linux::autostart::enabled()
}

pub(crate) fn set_autostart(on: bool) -> Result<(), String> {
    set_autostart_with(on, || !daemon::discover(Some(&pid_path())).is_empty())
}

pub(crate) fn set_autostart_with(on: bool, running: impl Fn() -> bool) -> Result<(), String> {
    let bin = if on {
        find_uscreen_bin().ok_or("uscreen binary not found")?
    } else {
        PathBuf::new()
    };
    uscreen_config::linux::autostart::set_enabled(on, &bin).map_err(|e| e.to_string())?;
    let result = if on {
        if !running() {
            start_daemon_with(service_managed_with(&running))
        } else {
            Ok(())
        }
    } else {
        run_daemon_command("stop", service_managed_with(&running))
    };
    result.map_err(|e| format!("Autostart preference saved; daemon action failed: {e}"))
}

pub(crate) fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

pub(crate) fn pid_path() -> PathBuf {
    PathBuf::from(format!("{}/.local/share/uscreen/uscreen.pid", home()))
}

#[cfg(test)]
pub(crate) fn apply_tablet_status(
    s: &mut Status,
    text: &str,
    sessions_path: Option<&std::path::Path>,
) {
    let sessions = sessions_path
        .and_then(uscreen_config::runtime::load_sessions)
        .unwrap_or_default();
    apply_tablet_sessions(s, text, sessions);
}

pub(crate) fn apply_tablet_sessions(
    s: &mut Status,
    text: &str,
    mut sessions: Vec<uscreen_config::runtime::TabletSession>,
) {
    sessions.sort_by_key(|session| session.instance);
    let models: Vec<_> = sessions
        .iter()
        .filter_map(|session| {
            let line = text.lines().find(|line| {
                let mut fields = line.split_whitespace();
                fields.next() == Some(session.serial.as_str()) && fields.next() == Some("device")
            })?;
            Some(
                line.split_whitespace()
                    .find_map(|field| field.strip_prefix("model:"))
                    .unwrap_or(&session.serial)
                    .replace('_', " "),
            )
        })
        .collect();
    s.tablet_connected = !models.is_empty();
    s.tablet_model = models.join(", ");
}

/// One-time privileged setup via the desktop's graphical password prompt:
/// pre-create an EVDI device now and at every boot.
pub(crate) fn system_setup_script(root: &std::path::Path, max_tablets: u32) -> String {
    let count = max_tablets.clamp(1, 4);
    let script = format!(
        r#"set -e
mkdir -p /etc/modprobe.d /etc/modules-load.d
echo 'options evdi initial_device_count={count}' > /etc/modprobe.d/uscreen-evdi.conf
printf 'evdi\nuinput\n' > /etc/modules-load.d/uscreen.conf
modprobe evdi || true
modprobe uinput || true
existing=$(cat /sys/devices/evdi/count 2>/dev/null || echo 0)
if [ "$existing" -lt {count} ]; then
    echo "$(({count} - existing))" > /sys/devices/evdi/add
fi"#
    );
    let script = format!("{}\nmkdir -p /etc/udev/rules.d\ncat > /etc/udev/rules.d/60-uscreen-uinput.rules <<'USCREEN_RULE'\n{}USCREEN_RULE\nudevadm control --reload\nudevadm trigger --name-match=uinput\n", script,
        include_str!("../../../packaging/60-uscreen-uinput.rules"));
    script
        .replace("/etc/", &format!("{}/etc/", root.display()))
        .replace("/sys/", &format!("{}/sys/", root.display()))
}

pub(crate) fn run_system_setup(max_tablets: u32) -> Result<(), String> {
    let script = system_setup_script(std::path::Path::new("/"), max_tablets);
    system_setup_result(
        Command::new("pkexec").args(["sh", "-c", &script]),
        Duration::from_secs(120),
    )
}

pub(crate) fn system_setup_result(command: &mut Command, timeout: Duration) -> Result<(), String> {
    let out = command
        .output_timeout(timeout)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                "Setup response timed out. Setup may still be running with elevated permissions. Wait and check its status before trying again.".to_owned()
            } else {
                format!("pkexec failed to run: {e}")
            }
        })?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Setup failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub(crate) fn daemon_command(bin: &std::path::Path, action: &str, managed: bool) -> Command {
    if managed {
        let mut command = Command::new("systemctl");
        command.args(["--user", action, "uscreen.service"]);
        command
    } else {
        let mut command = Command::new(bin);
        command.arg(action);
        command
    }
}

pub(crate) fn service_managed() -> bool {
    service_managed_with(|| !daemon::discover(Some(&pid_path())).is_empty())
}

pub(crate) fn service_managed_with(running: impl FnOnce() -> bool) -> bool {
    if !uscreen_config::linux::appimage::permits_service() {
        return false;
    }
    if Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "uscreen.service"])
        .output_bounded()
        .is_ok_and(|output| output.status.success())
    {
        return true;
    }
    if running() {
        return false;
    }
    uscreen_config::linux::autostart::systemd_available()
}

pub(crate) fn run_daemon_command(action: &str, managed: bool) -> Result<(), String> {
    let bin = if managed {
        PathBuf::new()
    } else {
        find_uscreen_bin().ok_or("uscreen binary not found")?
    };
    execute_daemon_command(action, &mut daemon_command(&bin, action, managed), managed)
}

pub(crate) fn execute_daemon_command(
    action: &str,
    command: &mut Command,
    managed: bool,
) -> Result<(), String> {
    let output = command
        .output_timeout(daemon_command_timeout(managed))
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} failed: {}",
            action,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

pub(crate) fn restart_daemon() -> Result<(), String> {
    restart_with(service_managed(), run_daemon_command, start_direct_daemon)
}

pub(crate) fn restart_with(
    managed: bool,
    mut run: impl FnMut(&str, bool) -> Result<(), String>,
    start: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if managed {
        return run("restart", true);
    }
    run("stop", false)?;
    start()
}

pub(crate) fn start_daemon() -> Result<(), String> {
    start_daemon_with(service_managed())
}

pub(crate) fn start_daemon_with(managed: bool) -> Result<(), String> {
    if managed {
        return run_daemon_command("start", true);
    }
    start_direct_daemon()
}

pub(crate) fn start_direct_daemon() -> Result<(), String> {
    let bin = find_uscreen_bin().ok_or("uscreen binary not found — run `make install`")?;
    let log_dir = PathBuf::from(format!("{}/.local/share/uscreen", home()));
    let _ = std::fs::create_dir_all(&log_dir);
    let log = std::fs::File::create(log_dir.join("daemon.log")).map_err(|e| e.to_string())?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;
    let mut child = daemon_command(&bin, "start", false)
        .stdout(log)
        .stderr(log_err)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start daemon: {}", e))?;
    // The GUI launches the foreground daemon as a child. The
    // std::process::Child handle must still be waited on or the kernel
    // leaves a zombie behind once it exits. Reap it on a background thread
    // instead of blocking the GUI.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub(crate) fn stop_daemon() -> Result<(), String> {
    run_daemon_command("stop", service_managed())
}

pub(crate) fn os_release_name() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_string())
            })
        })
        .unwrap_or_default()
        + " / "
        + &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()
        + " ("
        + &std::env::var("XDG_SESSION_TYPE").unwrap_or_default()
        + ")"
}
