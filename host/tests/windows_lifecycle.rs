//! T524: real Windows daemon processes, isolated from the developer's runtime.
#![cfg(windows)]
use blent_config::{
    commands::SyncCommandExt,
    windows::{private::Directory, runtime},
};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn command(path: &Path, action: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blent"));
    command.arg("--runtime-dir").arg(path).arg(action);
    command
}
struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn start(path: &Path) -> Daemon {
    Daemon(
        command(path, "start")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn wait_ready(path: &Path, child: &mut Daemon) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "T524: daemon exited before readiness"
        );
        if std::fs::read(path.join("ready.json"))
            .ok()
            .and_then(|bytes| {
                serde_json::from_slice::<blent_config::windows::process::Identity>(&bytes).ok()
            })
            .is_some_and(|owner| owner.pid == child.0.id())
        {
            return;
        }
        assert!(Instant::now() < deadline, "T524: startup timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn stop(path: &Path, child: &mut Daemon) {
    let output = command(path, "stop")
        .output_timeout(Duration::from_secs(12))
        .unwrap();
    assert!(output.status.success(), "T524: {output:?}");
    blent_config::lifecycle::wait_until(Duration::from_secs(5), || {
        Ok(child
            .0
            .try_wait()?
            .map(|status| {
                assert!(status.success());
                true
            })
            .unwrap_or(false))
    })
    .unwrap();
    for name in [
        "token",
        "sessions.json",
        "ready.json",
        "stop.json",
        "daemon.json",
    ] {
        assert!(!path.join(name).exists(), "T524: leaked {name}");
    }
}
#[test]
fn t524_concurrent_start_unicode_status_and_graceful_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime café 東京");
    let mut first = start(&path);
    wait_ready(&path, &mut first);
    let token = std::fs::read(path.join("token")).unwrap();
    let duplicate = command(&path, "start")
        .output_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(!duplicate.status.success());
    assert_eq!(std::fs::read(path.join("token")).unwrap(), token);
    let status = command(&path, "status")
        .output_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(status.status.success(), "{status:?}");
    assert!(String::from_utf8_lossy(&status.stdout).contains(&first.0.id().to_string()));
    assert!(String::from_utf8_lossy(&status.stdout).contains("unsupported"));
    std::fs::write(path.join("sessions.json"), b"[]").unwrap();
    stop(&path, &mut first);
    let absent = command(&path, "status")
        .output_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(absent.status.success());
    assert!(String::from_utf8_lossy(&absent.stdout).contains("stopped"));
}
#[test]
fn t524_crash_recovery_replaces_token_and_ignores_stale_stop() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let mut first = start(&path);
    wait_ready(&path, &mut first);
    let old_token = std::fs::read(path.join("token")).unwrap();
    let directory = Directory::create(&path).unwrap();
    let old_owner = runtime::owner_at(&directory).unwrap();
    first.0.kill().unwrap();
    first.0.wait().unwrap();
    assert!(runtime::owner_at(&directory).is_none());
    let mut next = start(&path);
    wait_ready(&path, &mut next);
    assert_ne!(std::fs::read(path.join("token")).unwrap(), old_token);
    std::fs::write(
        path.join("stop.json"),
        serde_json::to_vec(&old_owner).unwrap(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(next.0.try_wait().unwrap().is_none());
    stop(&path, &mut next);
}
#[test]
fn t524_stopped_status_and_stop_do_not_create_runtime() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("absent");
    for action in ["status", "stop"] {
        let result = command(&path, action)
            .output_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(result.status.success(), "T524: {result:?}");
        assert!(!path.exists());
    }
}

#[test]
fn t524_gui_controller_launch_restart_and_failed_start_retirement() {
    use blent_config::windows::lifecycle;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("controller café 東京");
    let program = Path::new(env!("CARGO_BIN_EXE_blent"));
    lifecycle::launch(program, &path, Duration::from_secs(10)).unwrap();
    let first = lifecycle::status(&path).unwrap().unwrap();
    // A successful concurrent launch converges on the existing owner.
    lifecycle::launch(&root.path().join("missing.exe"), &path, Duration::ZERO).unwrap();
    assert_eq!(lifecycle::status(&path).unwrap(), Some(first.clone()));
    blent_config::lifecycle::restart(
        || lifecycle::stop(&path, Duration::from_secs(5)).map_err(|e| e.to_string()),
        || lifecycle::launch(program, &path, Duration::from_secs(10)).map_err(|e| e.to_string()),
    )
    .unwrap();
    assert_ne!(lifecycle::status(&path).unwrap(), Some(first));
    lifecycle::stop(&path, Duration::from_secs(5)).unwrap();
    assert!(lifecycle::launch(
        &root.path().join("missing.exe"),
        &path,
        Duration::from_secs(1)
    )
    .is_err());
    assert!(lifecycle::status(&path).unwrap().is_none());
    assert!(lifecycle::launch(program, &path, Duration::ZERO).is_err());
    assert!(lifecycle::status(&path).unwrap().is_none());
    lifecycle::stop(&path, Duration::from_secs(5)).unwrap();
}

#[test]
fn t524_doctor_and_unsupported_operations_report_backend_truth() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("absent");
    let doctor = command(&path, "doctor")
        .output_timeout(Duration::from_secs(8))
        .unwrap();
    assert!(doctor.status.success(), "{doctor:?}");
    assert!(String::from_utf8_lossy(&doctor.stdout).contains("Display: unavailable (unsupported)"));
    let unsupported = command(&path, "list-displays")
        .output_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(!unsupported.status.success());
    assert!(String::from_utf8_lossy(&unsupported.stderr).contains("unsupported"));
    assert!(!path.exists());
}

#[test]
fn t524_default_invocation_uses_the_same_owned_lifecycle() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let mut child = Daemon(
        Command::new(env!("CARGO_BIN_EXE_blent"))
            .arg("--runtime-dir")
            .arg(&path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_ready(&path, &mut child);
    stop(&path, &mut child);
}

#[test]
fn t524_stop_reports_cleanup_failure_and_retires_other_resources() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let mut child = start(&path);
    wait_ready(&path, &mut child);
    std::fs::create_dir(path.join("sessions.json")).unwrap();
    let output = command(&path, "stop")
        .output_timeout(Duration::from_secs(8))
        .unwrap();
    assert!(
        !output.status.success(),
        "T524: stop falsely reported successful cleanup"
    );
    assert!(
        !path.join("token").exists(),
        "T524: stop retained token after a ledger failure"
    );
    blent_config::lifecycle::wait_until(Duration::from_secs(3), || {
        Ok(child
            .0
            .try_wait()?
            .map(|status| {
                assert!(!status.success());
                true
            })
            .unwrap_or(false))
    })
    .unwrap();
    assert!(path.join("sessions.json").is_dir());
}
