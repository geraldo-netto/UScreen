//! User autostart policy for systemd and XDG desktop sessions.
use crate::{commands::SyncCommandExt, storage::config_home};
use anyhow::{Context, Result};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

pub fn desktop_path() -> Result<PathBuf> {
    Ok(config_home()?.join("autostart/uscreen.desktop"))
}

pub fn systemd_available() -> bool {
    Command::new("systemctl")
        .args([
            "--user",
            "show",
            "-p",
            "LoadState",
            "--value",
            "uscreen.service",
        ])
        .output_bounded()
        .is_ok_and(|out| out.status.success() && out.stdout.trim_ascii() == b"loaded")
}

fn systemd_enabled() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "uscreen.service"])
        .output_bounded()
        .is_ok_and(|out| out.status.success() && out.stdout.trim_ascii() == b"enabled")
}

pub fn enabled() -> bool {
    systemd_enabled()
        || desktop_path()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .is_some_and(|text| desktop_enabled(&text))
}

fn desktop_enabled(text: &str) -> bool {
    let mut properties = std::collections::BTreeMap::new();
    let mut active = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            active = line == "[Desktop Entry]";
        } else if active {
            if let Some((key, value)) = line.split_once('=') {
                properties.insert(key.trim(), value.trim());
            }
        }
    }
    properties.get("Type") == Some(&"Application")
        && properties
            .get("Exec")
            .is_some_and(|value| !value.is_empty())
        && properties.get("Hidden") != Some(&"true")
}

fn desktop_argument(value: &str) -> String {
    let value = value
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('$', "\\\\$")
        .replace('`', "\\\\`")
        .replace('%', "%%")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{value}\"")
}

pub fn desktop_entry(binary: &Path) -> Result<String> {
    let path = std::path::absolute(binary).context("resolve autostart executable")?;
    let argument = desktop_argument(
        path.to_str()
            .context("desktop autostart needs a UTF-8 executable path")?,
    );
    let mut entry = String::new();
    for line in include_str!("../../../scripts/uscreen-autostart.desktop").lines() {
        if line.starts_with("Exec=") {
            entry.push_str(&format!("Exec=/usr/bin/env {argument} \"start\"\n"));
        } else {
            entry.push_str(line);
            entry.push('\n');
        }
    }
    Ok(entry)
}

fn remove_desktop_entry() -> Result<()> {
    match std::fs::remove_file(desktop_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("remove desktop autostart"),
    }
}

fn write_desktop_entry(binary: &Path) -> Result<()> {
    let entry = desktop_entry(binary)?;
    let path = desktop_path()?;
    let parent = path.parent().context("autostart directory")?;
    std::fs::create_dir_all(parent).context("create autostart directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(entry.as_bytes())?;
    file.persist(path).context("save desktop autostart")?;
    Ok(())
}

/// Persist the login preference. The GUI separately controls the current daemon.
pub fn set_enabled(on: bool, binary: &Path) -> Result<()> {
    if systemd_available() {
        let verb = if on { "enable" } else { "disable" };
        let out = Command::new("systemctl")
            .args(["--user", verb, "uscreen.service"])
            .output_bounded()
            .context("configure systemd autostart")?;
        anyhow::ensure!(
            out.status.success(),
            "systemctl {verb} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        // Avoid running both a desktop entry and a service on the next login.
        return remove_desktop_entry();
    }
    anyhow::ensure!(!systemd_enabled(), "The user service is enabled but its manager is unreachable; restore the user manager before changing autostart");
    if on {
        write_desktop_entry(binary)
    } else {
        remove_desktop_entry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn t231_shell_and_gui_entries_agree_and_launch_literal_paths() {
        let root = tempfile::tempdir().unwrap();
        let bin = root
            .path()
            .join("program space 'quote' \"double\" \\ $HOME %h `tick`\n\t");
        std::fs::write(
            &bin,
            "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\" > \"$USCREEN_T231_LAUNCH\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let shell = Command::new("bash")
            .arg(repo.join("scripts/write-desktop-entry.sh"))
            .arg(&bin)
            .arg(repo.join("scripts/uscreen-autostart.desktop"))
            .arg("start")
            .output()
            .unwrap();
        assert!(
            shell.status.success(),
            "{}",
            String::from_utf8_lossy(&shell.stderr)
        );
        let entry = desktop_entry(&bin).unwrap();
        assert_eq!(
            entry.as_bytes(),
            shell.stdout,
            "T231: installer/GUI entries diverged"
        );
        assert!(desktop_enabled(&entry));
        let path = root.path().join("uscreen.desktop");
        std::fs::write(&path, entry).unwrap();
        let marker = root.path().join("launched");
        let result = Command::new("gio")
            .args(["launch"])
            .arg(path)
            .env("USCREEN_T231_LAUNCH", &marker)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let expected = format!("{}\nstart\n", bin.display());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::fs::read_to_string(&marker).ok().as_ref() != Some(&expected)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(marker).unwrap(), expected);
    }

    #[test]
    fn t231_hidden_and_incomplete_entries_do_not_report_enabled() {
        let valid = "[Desktop Entry]\nType=Application\nExec=uscreen start\n";
        assert!(desktop_enabled(valid));
        for invalid in [
            String::new(),
            "[Other]\nType=Application\nExec=uscreen start".into(),
            "[Desktop Entry]\nType=Application\nExec=\n".into(),
            format!("{valid}Hidden=true\n"),
        ] {
            assert!(!desktop_enabled(&invalid));
        }
        assert!(desktop_enabled(&format!("{valid}[Other]\nHidden=true\n")));
    }
}
