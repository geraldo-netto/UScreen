//! T536: exercise real desktop entries against an isolated Cinnamon-like manager.
#![cfg(all(target_os = "linux", feature = "platform"))]
use blent_config::linux::autostart;
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration};

#[test]
fn t653_probe_child() {
    let Ok(expected) = std::env::var("BLENT_T653_EXPECT") else {
        return;
    };
    assert_eq!(autostart::systemd_available(), expected == "available");
    assert_eq!(autostart::enabled(), expected == "enabled");
}

#[test]
fn t656_disable_child() {
    if std::env::var_os("BLENT_T656_CHILD").is_none() {
        return;
    }
    let entry = autostart::desktop_path().unwrap();
    std::fs::create_dir_all(&entry).unwrap();
    std::fs::write(entry.join("unrelated"), "preserve").unwrap();
    assert!(autostart::set_enabled(false, Path::new("/fixture/blent")).is_err());
    assert_eq!(
        std::fs::read_to_string(entry.join("unrelated")).unwrap(),
        "preserve"
    );
    std::fs::remove_file(entry.join("unrelated")).unwrap();
    std::fs::remove_dir(&entry).unwrap();
    assert!(autostart::set_enabled(false, Path::new("/fixture/blent")).is_ok());
}

#[test]
fn t656_disable_reports_nonmissing_filesystem_errors() {
    let root = tempfile::tempdir().unwrap();
    let systemctl = root.path().join("systemctl");
    std::fs::write(&systemctl, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(systemctl, std::fs::Permissions::from_mode(0o700)).unwrap();
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "t656_disable_child", "--nocapture"])
        .env("PATH", root.path())
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env_remove(blent_config::linux::appimage::LAUNCHER)
        .env("BLENT_T656_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "T656: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn t653_systemd_probes_require_success_and_exact_state() {
    let root = tempfile::tempdir().unwrap();
    let systemctl = root.path().join("systemctl");
    std::fs::write(
        &systemctl,
        "#!/bin/sh\n/bin/cat \"$BLENT_T653_OUTPUT\"\nexit \"$BLENT_T653_EXIT\"\n",
    )
    .unwrap();
    std::fs::set_permissions(systemctl, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = root.path().join("reply");
    let cases: &[(&[u8], &str)] = &[
        (b"loaded", "available"),
        (b"\t loaded\r\n", "available"),
        (b"enabled", "enabled"),
        (b" enabled \n", "enabled"),
        (b"", "none"),
        (b"masked", "none"),
        (b"disabled", "none"),
        (b"not-found", "none"),
        (b"enabled\nextra", "none"),
        (b"loaded\0", "none"),
        (b"\xffenabled", "none"),
    ];
    for code in [0, 1, 3, 255] {
        for (text, state) in cases {
            std::fs::write(&output, text).unwrap();
            let expected = if code == 0 { *state } else { "none" };
            let result = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "t653_probe_child", "--nocapture"])
                .env("PATH", root.path())
                .env("HOME", root.path())
                .env("XDG_CONFIG_HOME", root.path().join("config"))
                .env_remove(blent_config::linux::appimage::LAUNCHER)
                .env("BLENT_T653_OUTPUT", &output)
                .env("BLENT_T653_EXIT", code.to_string())
                .env("BLENT_T653_EXPECT", expected)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "T653: {code}, {text:?}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}

fn wait_for_starts(state: &Path, expected: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let calls = std::fs::read_to_string(state.join("calls")).unwrap_or_default();
        let count = calls
            .lines()
            .filter(|line| *line == "--user start blent.service")
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
    let Some(state) = std::env::var_os("BLENT_T536_STATE") else {
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
    let binary = Path::new("/must-not-launch-a-direct-daemon/blent");
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
        .env("BLENT_T536_STATE", root.path())
        .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
        .env_remove(blent_config::linux::appimage::LAUNCHER)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
