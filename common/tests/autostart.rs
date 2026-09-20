//! T536: exercise real desktop entries against an isolated Cinnamon-like manager.
#![cfg(all(target_os = "linux", feature = "platform"))]
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration};
use uscreen_config::linux::autostart;

fn wait_for_starts(state: &Path, expected: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let calls = std::fs::read_to_string(state.join("calls")).unwrap_or_default();
        let count = calls
            .lines()
            .filter(|line| *line == "--user start uscreen.service")
            .count();
        if count >= expected && state.join("launches").exists() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "T536: desktop entry did not start the service: {calls}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn login(entry: &Path) {
    if entry.exists() {
        let output = Command::new("gio")
            .arg("launch")
            .arg(entry)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn t536_autostart_child() {
    let Some(state) = std::env::var_os("USCREEN_T536_STATE") else {
        return;
    };
    let state = Path::new(&state);
    assert!(!Command::new("systemctl")
        .args(["--user", "is-active", "graphical-session.target"])
        .status()
        .unwrap()
        .success());
    assert!(autostart::systemd_available());
    let entry = autostart::desktop_path().unwrap();
    let binary = Path::new("/must-not-launch-a-direct-daemon/uscreen");
    autostart::set_enabled(true, binary).unwrap();
    assert!(autostart::enabled());
    assert!(
        !state.join("launches").exists(),
        "T536: saving the preference launched a daemon"
    );
    assert!(
        entry.is_file(),
        "T536: enabled service has no desktop login route without graphical-session.target"
    );
    login(&entry);
    wait_for_starts(state, 1);
    autostart::set_enabled(true, binary).unwrap();
    login(&entry);
    wait_for_starts(state, 2);
    assert_eq!(
        std::fs::read_to_string(state.join("launches")).unwrap(),
        "daemon\n",
        "T536: repeated desktop/target startup created another daemon"
    );
    autostart::set_enabled(false, binary).unwrap();
    assert!(!autostart::enabled());
    assert!(!entry.exists());
    assert!(!state.join("enabled").exists());
    autostart::set_enabled(false, binary).unwrap();
    let calls = std::fs::read(state.join("calls")).unwrap();
    login(&entry);
    assert_eq!(
        std::fs::read(state.join("calls")).unwrap(),
        calls,
        "T536: disabled login invoked systemctl"
    );
}

#[test]
fn t536_managed_autostart_works_without_graphical_target_and_disables_cleanly() {
    let root = tempfile::tempdir().unwrap();
    let systemctl = root.path().join("systemctl");
    std::fs::write(
        &systemctl,
        include_str!("../../testdata/autostart_systemctl.sh"),
    )
    .unwrap();
    std::fs::set_permissions(systemctl, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "t536_autostart_child", "--nocapture"])
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config space"))
        .env("USCREEN_T536_STATE", root.path())
        .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
        .env_remove(uscreen_config::linux::appimage::LAUNCHER)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
