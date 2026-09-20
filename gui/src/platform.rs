//! GUI platform boundary. Capability flags describe implemented operations.
use std::path::PathBuf;
pub(crate) use uscreen_config::platform::{capabilities, programs::command_exists};
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;

fn installed_binary() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    return std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".local/bin/uscreen"));
    #[cfg(windows)]
    uscreen_config::platform::data_dir()
        .ok()
        .map(|dir| dir.join("uscreen.exe"))
}

pub(crate) fn publish_pipe(mib: u32) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return uscreen_config::linux::pipe::publish(mib).map_err(|error| error.to_string());
    #[cfg(windows)]
    {
        let _ = mib;
        Err("Pipe capacity is not implemented on Windows".into())
    }
}

pub(crate) fn find_uscreen_bin() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    match uscreen_config::linux::appimage::launcher() {
        Ok(Some(path)) => return Some(path),
        Err(_) => return None,
        Ok(None) => {}
    }
    find_uscreen_bin_in(
        std::env::current_exe().ok(),
        installed_binary().unwrap_or_default(),
        &std::env::var_os("PATH").unwrap_or_default(),
    )
}

pub(crate) fn find_uscreen_bin_in(
    exe: Option<PathBuf>,
    installed: PathBuf,
    path: &std::ffi::OsStr,
) -> Option<PathBuf> {
    use uscreen_config::platform::programs::{find_in, is_executable};
    if let Some(sibling) = exe.and_then(|exe| {
        exe.parent()
            .map(|dir| dir.join(uscreen_config::platform::executable_name("uscreen")))
    }) {
        if is_executable(&sibling) {
            return Some(sibling);
        }
    }
    find_in("uscreen", path).or_else(|| is_executable(&installed).then_some(installed))
}

#[cfg(all(test, target_os = "linux"))]
mod appimage_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn t541_pipe_publication_validates_and_preserves_previous_request() {
        if std::env::var_os("USCREEN_T541_PIPE_CHILD").is_some() {
            publish_pipe(4).unwrap();
            let path = uscreen_config::linux::pipe::request_path().unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "4\n");
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert!(publish_pipe(3)
                .unwrap_err()
                .contains("Invalid pipe capacity"));
            assert_eq!(std::fs::read_to_string(path).unwrap(), "4\n");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "platform::appimage_tests::t541_pipe_publication_validates_and_preserves_previous_request"])
            .env("USCREEN_T541_PIPE_CHILD", "1")
            .env("XDG_RUNTIME_DIR", root.path())
            .output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
    }

    #[test]
    fn t308_outer_launcher_child() {
        let Ok(mode) = std::env::var("USCREEN_T308_GUI_MODE") else {
            return;
        };
        let expected =
            std::env::var_os(uscreen_config::linux::appimage::LAUNCHER).map(PathBuf::from);
        match mode.as_str() {
            "valid" => assert_eq!(find_uscreen_bin(), expected),
            "invalid" => assert_eq!(find_uscreen_bin(), None),
            _ => {
                let _ = find_uscreen_bin();
            }
        }
    }

    #[test]
    fn t308_gui_uses_outer_image_and_refuses_moved_images() {
        let root = tempfile::tempdir().unwrap();
        let image = root.path().join("space %h$.AppImage");
        std::fs::write(&image, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o700)).unwrap();
        for (mode, path) in [
            ("valid", Some(image)),
            ("invalid", Some(root.path().join("moved"))),
            ("native", None),
        ] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "platform::appimage_tests::t308_outer_launcher_child",
                    "--nocapture",
                ])
                .env("USCREEN_T308_GUI_MODE", mode)
                .env_remove(uscreen_config::linux::appimage::LAUNCHER);
            if let Some(path) = path {
                command.env(uscreen_config::linux::appimage::LAUNCHER, path);
            }
            let result = command.output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stdout)
            );
        }
    }
}
