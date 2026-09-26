//! T497: adapter failures retain retry state without creating capture resources.
use super::*;
use std::os::unix::fs::PermissionsExt;

const TEST: &str =
    "monitor::coverage_tests::t497_reconnected_inventory_and_failed_extra_admission_preserve_state";

fn isolated() {
    let root = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env("BLENT_T497_MONITOR", "1")
        .env("HOME", root.path())
        .env("XDG_RUNTIME_DIR", root.path())
        .env("XDG_CONFIG_HOME", root.path())
        .env("PATH", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.path().join("blent/capture.fifo").exists());
}

#[tokio::test]
async fn t497_reconnected_inventory_and_failed_extra_admission_preserve_state() {
    if std::env::var_os("BLENT_T497_MONITOR").is_none() {
        isolated();
        return;
    }
    crate::test_logging::enable();
    let mut state = tests::fixture();
    for _ in 0..2 {
        state.inventory_ready(Inventory {
            devices: Some(vec![]),
            synthetic: vec![],
            reconnected: Some("192.0.2.8:5555".into()),
        });
    }
    assert!(state.wifi_announced);
    assert_eq!(
        crate::test_logging::text()
            .matches("Reconnected to the tablet over Wi-Fi")
            .count(),
        1
    );
    state.config.extra.video_port = u16::MAX;
    state.config.extra.input_port = u16::MAX;
    for _ in 0..2 {
        state.prepare_extra("fixture".into()).await;
    }
    assert!(state.extras.is_empty());
    assert!(state.pending.is_empty());
    assert!(!state.forwarding["fixture"].ready(Instant::now()));
    assert_eq!(
        crate::test_logging::text()
            .matches("Could not start tablet 2")
            .count(),
        1
    );
    state.stop().await;
}
