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
    let path = runtime_path()?;
    lifecycle::launch(&program, &path, Duration::from_secs(5)).map_err(|error| error.to_string())
}
pub(crate) fn stop_daemon() -> Result<(), String> {
    let path = runtime_path()?;
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

// Tests supply a private runtime to the same native adapter calls. Never point
// a lifecycle regression at the developer's running application.
#[cfg(test)]
thread_local! {
    static TEST_RUNTIME: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}
pub(crate) fn runtime_path() -> Result<std::path::PathBuf, String> {
    #[cfg(test)]
    if let Some(path) = TEST_RUNTIME.with(|value| value.borrow().clone()) {
        return Ok(path);
    }
    runtime::runtime_dir().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t524_gui_routes_lifecycle_and_preserves_startup_errors() {
        if let Some(path) = std::env::var_os("BLENT_T524_GUI_RUNTIME") {
            TEST_RUNTIME.with(|value| *value.borrow_mut() = Some(path.into()));
            assert!(start_daemon()
                .unwrap_err()
                .contains("daemon exited before readiness"));
            stop_daemon().unwrap();
            assert!(restart_daemon()
                .unwrap_err()
                .contains("daemon exited before readiness"));
            stop_daemon().unwrap();
            assert!(!autostart_enabled());
            assert!(os_release_name().contains("Windows"));
            return;
        }
        let root = tempfile::tempdir().unwrap();
        // A native test executable deliberately rejects the daemon CLI. This
        // exercises the GUI error path; host integration tests cover success.
        std::fs::copy(
            std::env::current_exe().unwrap(),
            root.path().join("blent.exe"),
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "platform::windows::tests::t524_gui_routes_lifecycle_and_preserves_startup_errors",
                "--nocapture",
            ])
            .env(
                "BLENT_T524_GUI_RUNTIME",
                root.path().join("runtime café 東京"),
            )
            .env("PATH", root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
