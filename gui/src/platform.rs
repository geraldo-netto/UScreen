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
