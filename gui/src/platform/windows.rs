//! Interactive-user Windows lifecycle and per-user login preferences.
use blent_config::windows::{autostart, lifecycle, runtime};
use std::time::Duration;
fn unsupported() -> Result<(), String> {
    Err("System setup is not implemented on Windows".into())
}
pub(crate) fn autostart_enabled() -> bool {
    autostart::Registration::at(&autostart_key())
        .enabled()
        .unwrap_or(false)
}
pub(crate) fn set_autostart(on: bool) -> Result<(), String> {
    autostart::Registration::at(&autostart_key())
        .set_enabled(on, super::find_blent_bin().as_deref())
        .map_err(|error| error.to_string())
}
fn autostart_key() -> String {
    #[cfg(test)]
    if let Some(path) = TEST_AUTOSTART.with(|value| value.borrow().clone()) {
        return path;
    }
    autostart::RUN_KEY.into()
}
pub(crate) fn start_daemon() -> Result<(), String> {
    let program = super::find_blent_bin().ok_or("blent binary not found")?;
    let path = runtime_path()?;
    lifecycle::launch(&program, &path, Duration::from_secs(5)).map_err(|error| error.to_string())
}
pub(crate) fn stop_daemon() -> Result<(), String> {
    let path = runtime_path()?;
    lifecycle::stop(&path, lifecycle::STOP_TIMEOUT).map_err(|error| error.to_string())
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
    static TEST_AUTOSTART: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}
pub(crate) fn runtime_path() -> Result<std::path::PathBuf, String> {
    #[cfg(test)]
    if let Some(path) = TEST_RUNTIME.with(|value| value.borrow().clone()) {
        return Ok(path);
    }
    runtime::runtime_dir().map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "registry_fixture.rs"]
mod registry_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use blent_config::commands::SyncCommandExt;
    #[test]
    fn t532_gui_preference_tracks_registration_and_stale_executable() {
        if let Some(key) = std::env::var_os("BLENT_T532_GUI_KEY") {
            let key = key.to_string_lossy();
            let _registry = registry_fixture::RegistryFixture::redirect(&key);
            TEST_AUTOSTART.with(|value| *value.borrow_mut() = Some(key.to_string()));
            set_autostart(false).unwrap();
            assert!(!autostart_enabled());
            set_autostart(true).unwrap();
            assert!(autostart_enabled());
            // T660: do not ask the mutated helper where it wrote registration.
            assert!(autostart::Registration::at(&key).enabled().unwrap());
            let program = super::super::find_blent_bin().unwrap();
            std::fs::remove_file(program).unwrap();
            assert!(!autostart_enabled());
            set_autostart(false).unwrap();
            assert!(!autostart_enabled());
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let key = format!(
            "Software\\BlentTests\\GuiT532-{}",
            blent_config::credentials::random_token().unwrap()
        );
        let runner = root.path().join("gui-tests.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &runner).unwrap();
        std::fs::copy(&runner, root.path().join("blent.exe")).unwrap();
        let result = std::process::Command::new(runner)
            .args(["--exact", "platform::windows::tests::t532_gui_preference_tracks_registration_and_stale_executable", "--nocapture"])
            .env("BLENT_T532_GUI_KEY", &key)
            .env("PATH", root.path())
            .output_timeout(Duration::from_secs(15)).unwrap();
        let reg = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/reg.exe");
        let cleanup = std::process::Command::new(reg)
            .args(["delete", &format!("HKCU\\{key}"), "/f"])
            .output_bounded()
            .unwrap();
        assert!(
            cleanup.status.success(),
            "T532: fixture cleanup failed: {cleanup:?}"
        );
        assert!(result.status.success(), "T532: {result:?}");
    }
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
        // Run the GUI fixture beside its private fake daemon; the build's
        // deps directory can contain the real blent.exe and wins over PATH.
        let runner = root.path().join("gui-tests.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &runner).unwrap();
        let output = std::process::Command::new(runner)
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
            .output_timeout(Duration::from_secs(12))
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
