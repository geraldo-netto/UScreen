//! T330: real daemon helper commands and C allocation, entirely fake DRM.
use super::*;
use std::path::{Path, PathBuf};

fn compile_helper(root: &Path) -> PathBuf {
    let binary = root.join("evdi_helper");
    let output = std::process::Command::new("cc")
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-ffunction-sections",
            "-fdata-sections",
            "-Wl,--gc-sections",
            "-pthread",
        ])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/evdi_helper_test.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "T330 fixture compile: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn card(root: &Path, number: u32, connected: bool) {
    let connector = root.join(format!(
        "evdi.{number}/drm/card{number}/card{number}-DVI-I-1"
    ));
    std::fs::create_dir_all(&connector).unwrap();
    std::fs::write(
        connector.join("status"),
        if connected {
            "connected\n"
        } else {
            "disconnected\n"
        },
    )
    .unwrap();
    std::fs::write(root.join(format!("card{number}")), "fake DRM inode").unwrap();
}

fn manager(root: &Path, instance: u32) -> CaptureManager {
    let config = CaptureConfig {
        helper_path: root.join("evdi_helper"),
        edid_path: Some(root.join("unused.edid")),
        encoder: "libx264".into(),
        instance,
        ..CaptureConfig::default()
    };
    // Same production allocator invoked by multi-tablet daemon construction.
    CaptureManager::new(crate::slot_capture_config(config, instance))
}

async fn reconnect_prefers_its_previous_card(root: &Path, manager: &mut CaptureManager) {
    let previous = manager.helper.card.unwrap();
    manager.shutdown().await;
    card(root, 1, false); // A new, lower-numbered free card must not reshuffle a session.
    manager.start_helper().await.unwrap();
    assert_eq!(
        manager.helper.card,
        Some(previous),
        "T330 avoid avoidable card churn"
    );
    manager.shutdown().await;
    // Its DRM index still exists but no longer belongs to EVDI (e.g. reused
    // by another driver). A stale preference must not open that inode.
    std::fs::remove_dir_all(root.join(format!("evdi.{previous}"))).unwrap();
    manager.start_helper().await.unwrap();
    assert_eq!(
        manager.helper.card,
        Some(1),
        "T330 removed card stranded its session"
    );
}

async fn occupied_preference_is_not_stolen(root: &Path, manager: &mut CaptureManager) {
    let previous = manager.helper.card.unwrap();
    manager.shutdown().await;
    card(root, previous, true); // Another application acquired it while we were stopped.
    card(root, 8, false);
    manager.start_helper().await.unwrap();
    assert_eq!(
        manager.helper.card,
        Some(8),
        "T330 occupied preference stranded a session"
    );
    let status = root.join(format!(
        "evdi.{previous}/drm/card{previous}/card{previous}-DVI-I-1/status"
    ));
    assert_eq!(std::fs::read_to_string(status).unwrap(), "connected\n");
}

async fn allocation_lifecycle(root: &Path) {
    for (number, busy) in [(0, true), (2, false), (4, false)] {
        card(root, number, busy);
    }
    let mut primary = manager(root, 0);
    let mut secondary = manager(root, 1);
    let (first, second) = tokio::join!(primary.start_helper(), secondary.start_helper());
    first.expect("T330 primary pinned to an occupied card despite free capacity");
    second.unwrap();
    let actual = std::collections::BTreeSet::from([
        primary.helper.card.unwrap(),
        secondary.helper.card.unwrap(),
    ]);
    assert_eq!(actual, std::collections::BTreeSet::from([2, 4]));
    let mut extra = manager(root, 2);
    assert!(
        extra.start_helper().await.is_err(),
        "T330 exhausted pool stole an active card"
    );
    reconnect_prefers_its_previous_card(root, &mut primary).await;
    let second_card = secondary.helper.card;
    secondary.shutdown().await;
    secondary.start_helper().await.unwrap();
    assert_eq!(secondary.helper.card, second_card);
    card(root, 6, false);
    extra.start_helper().await.unwrap();
    assert_eq!(
        extra.helper.card,
        Some(6),
        "T330 new capacity was not discovered"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("evdi.0/drm/card0/card0-DVI-I-1/status")).unwrap(),
        "connected\n"
    );
    occupied_preference_is_not_stolen(root, &mut secondary).await;
    primary.shutdown().await;
    secondary.shutdown().await;
    extra.shutdown().await;
}

#[tokio::test]
async fn t330_daemon_slots_share_free_card_leases_across_restarts() {
    if let Some(root) = std::env::var_os("USCREEN_T330_ROOT") {
        allocation_lifecycle(Path::new(&root)).await;
        return;
    }
    let root = tempfile::tempdir().unwrap();
    compile_helper(root.path());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "capture::card_allocation_tests::t330_daemon_slots_share_free_card_leases_across_restarts", "--nocapture"])
        .env("USCREEN_T330_ROOT", root.path()).env("USCREEN_T330_DRM", root.path())
        .env("XDG_RUNTIME_DIR", root.path()).env("HOME", root.path()).output().unwrap();
    assert!(
        output.status.success(),
        "T330: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
