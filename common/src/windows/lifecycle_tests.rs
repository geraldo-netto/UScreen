//! T524 native ownership, malformed-state bounds and cancellation regressions.
use super::*;
use std::process::Stdio;

#[test]
fn t524_cancellation_child() {
    let Ok(path) = std::env::var("BLENT_T524_CHILD") else {
        return;
    };
    std::fs::write(path, b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

#[tokio::test]
async fn t524_cancel_retires_session_and_owned_child() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let session = Session::start(&path).unwrap();
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "windows::lifecycle::tests::t524_cancellation_child",
            "--nocapture",
        ])
        .env("BLENT_T524_CHILD", root.path().join("child-ready"))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = crate::commands::OwnedChild::spawn(&mut command).unwrap();
    let identity = Identity::read(child.id().unwrap()).unwrap();
    let task = tokio::spawn(async move {
        let _session = session;
        let _child = child;
        std::future::pending::<()>().await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    crate::lifecycle::wait_until(Duration::from_secs(3), || Ok(!identity.is_current())).unwrap();
    for name in STATE {
        assert!(!path.join(name).exists(), "T524: leaked {name}");
    }
    assert!(!path.join("daemon.json").exists());
}

#[test]
fn t524_partial_start_and_malformed_requests_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let directory = Directory::create(&path).unwrap();
    std::fs::create_dir(path.join("token")).unwrap();
    assert!(Session::start(&path).is_err());
    assert!(!path.join("daemon.json").exists());
    std::fs::remove_dir(path.join("token")).unwrap();
    let session = Session::start(&path).unwrap();
    assert!(!session.stop_requested());
    for size in [0, 1, 63, 64, 65535, 65536, 65537, 100000] {
        std::fs::write(path.join("stop.json"), vec![b'{'; size]).unwrap();
        assert!(!session.stop_requested());
    }
    let mut wrong = session.lease.identity().clone();
    wrong.started = wrong.started.wrapping_add(1);
    publish(&path, "stop.json", &wrong).unwrap();
    assert!(!session.stop_requested());
    assert!(stop(&path, Duration::ZERO).is_err());
    assert!(session.stop_requested());
    // A live corrupt owner must not be treated as abandoned state.
    std::fs::write(path.join("daemon.json"), b"corrupt").unwrap();
    assert!(stop(&path, Duration::ZERO).is_err());
    assert!(path.join("token").exists());
    // Restore owner so ordinary retirement can clean up its own state.
    publish(&path, "daemon.json", session.lease.identity()).unwrap();
    drop(session);
    assert!(runtime::owner_at(&directory).is_none());
    stop(&path, Duration::ZERO).unwrap();
    assert!(status(Path::new("relative")).is_err());
    assert!(Directory::open(Path::new("relative")).is_err());
    let missing = root.path().join("missing");
    assert!(Directory::open(&missing).is_err());
    assert!(!missing.exists());
    let file = root.path().join("file");
    std::fs::write(&file, b"not a directory").unwrap();
    assert!(status(&file).is_err());
}

#[test]
fn t524_drop_preserves_replaced_state_and_crash_recovery_cleans_abandoned_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let session = Session::start(&path).unwrap();
    std::fs::write(path.join("daemon.json"), b"replacement").unwrap();
    drop(session);
    assert!(path.join("token").exists());
    std::fs::write(path.join("sessions.json"), b"stale").unwrap();
    stop(&path, Duration::ZERO).unwrap();
    for name in STATE {
        assert!(!path.join(name).exists());
    }
    assert!(!path.join("daemon.json").exists());
}

#[test]
fn t524_cleanup_failure_still_retires_token_and_reports_failed_stop() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let session = Session::start(&path).unwrap();
    // A blocked session ledger must not prevent independent token retirement.
    std::fs::create_dir(path.join("sessions.json")).unwrap();
    drop(session);
    assert!(
        !path.join("token").exists(),
        "T524: unrelated cleanup failure retained the authentication token"
    );
    assert!(!path.join("ready.json").exists());
    assert!(stop(&path, Duration::ZERO).is_err());
    assert!(path.join("sessions.json").is_dir());
}

#[test]
fn t524_explicit_shutdown_preserves_replacement_state() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    let session = Session::start(&path).unwrap();
    let token = std::fs::read(path.join("token")).unwrap();
    std::fs::write(path.join("daemon.json"), b"replacement").unwrap();
    assert!(session
        .shutdown()
        .unwrap_err()
        .to_string()
        .contains("ownership changed"));
    assert_eq!(std::fs::read(path.join("token")).unwrap(), token);
    assert_eq!(
        std::fs::read(path.join("daemon.json")).unwrap(),
        b"replacement"
    );
    stop(&path, Duration::ZERO).unwrap();
}

#[test]
fn t525_session_status_requires_current_owner_and_bounded_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime");
    assert_eq!(load_sessions(&path), None);
    let session = Session::start(&path).unwrap();
    assert_eq!(load_sessions(&path), None);
    let tablets = vec![crate::tablets::TabletSession {
        serial: "USB".into(),
        instance: 0,
        video_port: 8890,
        input_port: 8891,
    }];
    session.publish_sessions(&tablets).unwrap();
    assert_eq!(load_sessions(&path), Some(tablets.clone()));
    for size in [0, 1, 64, 65536, 65537] {
        std::fs::write(path.join("sessions.json"), vec![b'{'; size]).unwrap();
        assert_eq!(load_sessions(&path), None);
    }
    let mut wrong = session.lease.identity().clone();
    wrong.started += 1;
    publish(
        &path,
        "sessions.json",
        &Snapshot {
            owner: wrong,
            sessions: tablets.clone(),
        },
    )
    .unwrap();
    assert_eq!(load_sessions(&path), None);
    session.publish_sessions(&tablets).unwrap();
    std::fs::write(path.join("daemon.json"), b"replacement").unwrap();
    assert!(session.publish_sessions(&[]).is_err());
    assert_eq!(load_sessions(&path), None);
}
