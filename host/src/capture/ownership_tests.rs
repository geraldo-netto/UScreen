//! T429: capture cleanup must never unlink another manager's live FIFO.
use super::*;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

fn isolated(name: &str) -> bool {
    if std::env::var_os("USCREEN_T429_RUNTIME").is_none() {
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("USCREEN_T429_RUNTIME", "1")
            .env("XDG_RUNTIME_DIR", root.path())
            .env("HOME", root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T429: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return true;
    }
    false
}

#[tokio::test]
async fn t429_unstarted_managers_preserve_live_fifo() {
    if isolated("capture::ownership_tests::t429_unstarted_managers_preserve_live_fifo") {
        return;
    }
    let path = fifo_path_for(0).unwrap();
    for explicit_shutdown in [false, true] {
        helper::ensure_fifo(&path).unwrap();
        let original = std::fs::symlink_metadata(&path).unwrap();
        let mut observer = CaptureManager::new(Default::default());
        if explicit_shutdown {
            observer.shutdown().await;
        }
        drop(observer);
        let retained = std::fs::symlink_metadata(&path)
            .expect("T429: an unstarted manager removed another capture's FIFO");
        assert!(retained.file_type().is_fifo());
        assert_eq!(
            (retained.dev(), retained.ino()),
            (original.dev(), original.ino())
        );
    }
}

async fn owner() -> CaptureManager {
    let root = crate::runtime::runtime_dir().unwrap();
    let helper = root.join("helper");
    std::fs::write(
        &helper,
        "#!/bin/sh\necho 'EVDI_CONNECTED card4294967295'\nexec sleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut manager = CaptureManager::new(CaptureConfig {
        encoder: "libx264".into(),
        helper_path: helper,
        edid_path: Some(root.join("unused.edid")),
        ..Default::default()
    });
    manager.start_helper().await.unwrap();
    manager
}

// T499 investigation: ensure_fifo already creates a distinct inode. Preserve
// that behavior across repeated helper starts; no production fix was needed.
#[tokio::test]
async fn t499_helper_restart_keeps_its_new_fifo() {
    use std::os::unix::fs::OpenOptionsExt;
    if isolated("capture::ownership_tests::t499_helper_restart_keeps_its_new_fifo") {
        return;
    }
    let path = fifo_path_for(0).unwrap();
    let mut manager = owner().await;
    for _ in 0..3 {
        let pin = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH)
            .open(&path)
            .unwrap();
        manager.helper.terminate().await;
        manager.start_helper().await.unwrap();
        let current = std::fs::symlink_metadata(&path)
            .expect("T499: previous ownership unlinked the restarted helper's FIFO");
        assert!(current.file_type().is_fifo());
        assert_ne!(current.ino(), pin.metadata().unwrap().ino());
    }
    manager.shutdown().await;
    assert!(!path.exists());
}

#[tokio::test]
async fn t429_retired_owner_preserves_replacement_paths() {
    use std::os::unix::fs::{symlink, OpenOptionsExt};
    if isolated("capture::ownership_tests::t429_retired_owner_preserves_replacement_paths") {
        return;
    }
    let path = fifo_path_for(0).unwrap();
    for kind in 0..3 {
        let mut manager = owner().await;
        let pin = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH)
            .open(&path)
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        match kind {
            0 => helper::ensure_fifo(&path).unwrap(),
            1 => std::fs::write(&path, "replacement").unwrap(),
            _ => symlink("unrelated-target", &path).unwrap(),
        }
        let replacement = std::fs::symlink_metadata(&path).unwrap();
        manager.shutdown().await;
        drop(manager);
        let retained = std::fs::symlink_metadata(&path)
            .expect("T429: retired manager removed a replacement path");
        assert_eq!(
            (retained.dev(), retained.ino()),
            (replacement.dev(), replacement.ino())
        );
        drop(pin);
    }
}

#[tokio::test]
async fn t429_shutdown_releases_fifo_ownership_once() {
    if isolated("capture::ownership_tests::t429_shutdown_releases_fifo_ownership_once") {
        return;
    }
    let path = fifo_path_for(0).unwrap();
    let mut manager = owner().await;
    manager.shutdown().await;
    assert!(!path.exists(), "T429: owned FIFO was not cleaned up");
    helper::ensure_fifo(&path).unwrap();
    drop(manager);
    assert!(
        path.exists(),
        "T429: Drop removed the next owner's FIFO after shutdown"
    );
}

#[test]
fn t429_ownership_does_not_hold_fifo_endpoints_open() {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("frames");
    let owned = fifo::Owned::create(&path).unwrap();
    let error = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&path)
        .expect_err("T429: ownership must not count as a FIFO reader");
    assert_eq!(error.raw_os_error(), Some(libc::ENXIO));
    let mut reader = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&path)
        .unwrap();
    assert_eq!(
        reader.read(&mut [0u8; 1]).unwrap(),
        0,
        "T429: ownership must not count as a FIFO writer or hide EOF"
    );
    drop(owned);
    assert!(!path.exists(), "T429: owned FIFO leaked on Drop");
}
