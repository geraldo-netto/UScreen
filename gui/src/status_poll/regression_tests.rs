//! T516: an unsuccessful ADB inventory cannot claim a connected tablet.
use super::*;
use std::os::unix::fs::PermissionsExt;
use uscreen_config::runtime::{runtime_dir, SessionLedger, TabletSession};

const TEST: &str =
    "status_poll::regression_tests::t516_dynamic_inventory_requires_success_and_recovers";

fn isolated() {
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    std::fs::create_dir(directory.path().join("bin")).unwrap();
    let executable = directory.path().join("bin/uscreen");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let adb = directory.path().join("adb");
    std::fs::write(&adb, "#!/bin/sh\n/bin/cat \"$HOME/inventory\"\nread -r status < \"$HOME/status\"\nexit \"$status\"\n").unwrap();
    std::fs::set_permissions(adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = std::process::Command::new(executable)
        .args(["--exact", TEST, "--nocapture"])
        .env("USCREEN_T516", "1")
        .env("HOME", directory.path())
        .env("XDG_CONFIG_HOME", directory.path())
        .env("XDG_RUNTIME_DIR", directory.path())
        .env("PATH", directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t516_dynamic_inventory_requires_success_and_recovers() {
    if std::env::var_os("USCREEN_T516").is_none() {
        isolated();
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    std::fs::write(
        root.join("inventory"),
        "List of devices attached\nTABLET device model:Fixture_Tablet\n",
    )
    .unwrap();
    let path = runtime_dir().unwrap().join("sessions.json");
    let mut ledger = SessionLedger::new(path.clone()).unwrap();
    ledger
        .update(vec![TabletSession {
            serial: "TABLET".into(),
            instance: 0,
            video_port: 19000,
            input_port: 20000,
        }])
        .unwrap();
    let saved = std::fs::read(&path).unwrap();
    let capabilities = Capabilities {
        adb: true,
        daemon_binary: false,
        ffmpeg: false,
        autostart: false,
    };
    let mut source = linux::Platform::default();
    for (code, connected) in [(17, false), (0, true), (1, false), (0, true)] {
        std::fs::write(root.join("status"), format!("{code}\n")).unwrap();
        let status = source.dynamic(&capabilities);
        assert_eq!(
            status.tablet_connected, connected,
            "T516 failed inventory must not supply live device evidence"
        );
        assert_eq!(
            status.tablet_model,
            if connected { "Fixture Tablet" } else { "" }
        );
        assert_eq!(std::fs::read(&path).unwrap(), saved);
    }
}
