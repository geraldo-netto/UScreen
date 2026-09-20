//! Shared service entry points. Capabilities mean implemented backends, not
//! successful permission, device or driver detection on the current machine.
#[cfg(not(windows))]
use anyhow::Context;
use anyhow::Result;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub use crate::linux::{programs, runtime};
#[cfg(windows)]
pub use crate::windows::{programs, runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub camera: bool,
    pub daemon: bool,
    pub display: bool,
    pub input: bool,
    pub system_setup: bool,
    pub autostart: bool,
    pub pipe_capacity: bool,
    pub conversion_pool: bool,
}

pub const fn capabilities() -> Capabilities {
    let linux = cfg!(target_os = "linux");
    Capabilities {
        camera: linux,
        daemon: linux,
        display: linux,
        input: linux,
        system_setup: linux,
        autostart: linux,
        pipe_capacity: linux,
        conversion_pool: linux,
    }
}

pub fn data_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    return Ok(crate::windows::paths::local()?.join("uscreen"));
    #[cfg(not(windows))]
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|home| home.join(".local/share/uscreen"))
        .context("data location needs an absolute HOME")
}

pub fn executable_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t493_capabilities_never_imply_an_unimplemented_backend() {
        let caps = capabilities();
        assert_eq!(caps.camera, cfg!(target_os = "linux"));
        assert_eq!(caps.daemon, cfg!(target_os = "linux"));
        assert_eq!(caps.display, caps.daemon);
        assert_eq!(caps.input, caps.daemon);
        assert_eq!(caps.system_setup, caps.daemon);
        assert_eq!(caps.autostart, caps.daemon);
        assert_eq!(caps.pipe_capacity, caps.daemon);
        assert_eq!(caps.conversion_pool, caps.daemon);
        assert_eq!(
            executable_name("uscreen"),
            if cfg!(windows) {
                "uscreen.exe"
            } else {
                "uscreen"
            }
        );
        assert!(data_dir().unwrap().is_absolute());
    }
}
