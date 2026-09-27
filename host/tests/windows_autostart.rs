//! T532: login launches the ordinary per-user daemon once, without elevation.
#![cfg(windows)]
use blent_config::{commands::SyncCommandExt, windows::lifecycle};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

struct OwnedRuntime(PathBuf);
impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        let _ = lifecycle::stop(&self.0, Duration::from_secs(5));
    }
}

#[test]
fn t532_repeated_login_preserves_daemon_identity_and_token() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("login café 東京");
    std::fs::create_dir(&directory).unwrap();
    let program = directory.join("blent.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_blent"), &program).unwrap();
    let runtime = OwnedRuntime(root.path().join("runtime"));
    let login = || {
        Command::new(&program)
            .arg("--runtime-dir")
            .arg(&runtime.0)
            .arg("--login")
            // Explorer's Run launcher does not put the daemon in a temporary
            // kill-on-close command job. Simulate that native login boundary.
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
    };
    let result = login();
    assert!(result.success(), "T532: login failed: {result:?}");
    let first = lifecycle::status(&runtime.0).unwrap().unwrap();
    let token = std::fs::read(runtime.0.join("token")).unwrap();
    for _ in 0..3 {
        let result = login();
        assert!(result.success(), "T532: repeated login failed: {result:?}");
        assert_eq!(lifecycle::status(&runtime.0).unwrap(), Some(first.clone()));
        assert_eq!(std::fs::read(runtime.0.join("token")).unwrap(), token);
    }
    let contradictory = Command::new(&program)
        .arg("--runtime-dir")
        .arg(&runtime.0)
        .args(["--login", "stop"])
        .output_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(!contradictory.status.success());
    assert_eq!(lifecycle::status(&runtime.0).unwrap(), Some(first));
}
