//! Stable outer invocation supplied by AppRun, never a temporary mount path.
use crate::commands::SyncCommandExt;
use anyhow::{ensure, Result};
use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
};

pub const LAUNCHER: &str = "USCREEN_APPIMAGE_LAUNCHER";

pub fn launcher() -> Result<Option<PathBuf>> {
    std::env::var_os(LAUNCHER)
        .as_deref()
        .map(checked_launcher)
        .transpose()
}

fn checked_launcher(value: &OsStr) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    ensure!(
        path.is_absolute() && super::programs::is_executable(&path),
        "AppImage launcher moved or is not executable; relaunch from its stable location"
    );
    Ok(path)
}

pub fn marker(path: &Path) -> String {
    let encoded: String = path
        .as_os_str()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("# USCREEN_APPIMAGE_PATH_HEX={encoded}")
}

pub fn unit_matches(path: &Path, text: &str) -> bool {
    let expected = marker(path);
    text.lines().any(|line| line == expected)
}

pub fn permits_service() -> bool {
    match launcher() {
        Ok(None) => true,
        Ok(Some(path)) => service_matches(&path),
        Err(_) => false,
    }
}

fn service_matches(path: &Path) -> bool {
    let unit = Command::new("systemctl")
        .args([
            "--user",
            "show",
            "-p",
            "FragmentPath",
            "--value",
            "uscreen.service",
        ])
        .output_bounded();
    let Ok(unit) = unit else {
        return false;
    };
    if !unit.status.success() {
        return false;
    }
    let file = String::from_utf8_lossy(&unit.stdout);
    std::fs::read_to_string(file.trim()).is_ok_and(|text| unit_matches(path, &text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn t308_stable_launcher_requires_absolute_executable_and_literal_unit_binding() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("UScreen space % $ ` ' \"\n.AppImage");
        for path in [Path::new("relative"), image.as_path(), dir.path()] {
            assert!(checked_launcher(path.as_os_str()).is_err());
        }
        std::fs::write(&image, "#!/bin/sh\nexit 0\n").unwrap();
        assert!(checked_launcher(image.as_os_str()).is_err());
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(checked_launcher(image.as_os_str()).unwrap(), image);
        let text = format!("[Unit]\n{}\n[Service]\n", marker(&image));
        assert!(unit_matches(&image, &text));
        assert!(!unit_matches(&dir.path().join("another.AppImage"), &text));
        assert!(!unit_matches(
            &image,
            "[Service]\nExecStart=/usr/bin/uscreen start\n"
        ));
    }
}
