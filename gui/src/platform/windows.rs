//! Interactive-user Windows lifecycle; setup and autostart remain unsupported.
use blent_config::windows::{lifecycle, runtime};
use std::time::Duration;
fn unsupported() -> Result<(), String> {
    Err("System setup and autostart are not implemented on Windows".into())
}
pub(crate) fn autostart_enabled() -> bool {
    false
}
pub(crate) fn set_autostart(_on: bool) -> Result<(), String> {
    unsupported()
}
pub(crate) fn start_daemon() -> Result<(), String> {
    let program = super::find_blent_bin().ok_or("blent binary not found")?;
    let path = runtime::runtime_dir().map_err(|error| error.to_string())?;
    lifecycle::launch(&program, &path, Duration::from_secs(5)).map_err(|error| error.to_string())
}
pub(crate) fn stop_daemon() -> Result<(), String> {
    let path = runtime::runtime_dir().map_err(|error| error.to_string())?;
    lifecycle::stop(&path, Duration::from_secs(5)).map_err(|error| error.to_string())
}
pub(crate) fn restart_daemon() -> Result<(), String> {
    blent_config::lifecycle::restart(stop_daemon, start_daemon)
}
pub(crate) fn run_system_setup(_max_tablets: u32) -> Result<(), String> {
    unsupported()
}
pub(crate) fn os_release_name() -> String {
    format!("Windows / {}", std::env::consts::ARCH)
}
