//! Explicitly unsupported operations until a Windows runtime backend exists.
fn unsupported() -> Result<(), String> {
    Err("Daemon lifecycle and system setup are not implemented on Windows".into())
}
pub(crate) fn autostart_enabled() -> bool {
    false
}
pub(crate) fn set_autostart(_on: bool) -> Result<(), String> {
    unsupported()
}
pub(crate) fn start_daemon() -> Result<(), String> {
    unsupported()
}
pub(crate) fn stop_daemon() -> Result<(), String> {
    unsupported()
}
pub(crate) fn restart_daemon() -> Result<(), String> {
    unsupported()
}
pub(crate) fn run_system_setup(_max_tablets: u32) -> Result<(), String> {
    unsupported()
}
pub(crate) fn os_release_name() -> String {
    format!("Windows / {}", std::env::consts::ARCH)
}
