//! T497: mapping admission and retries use private command adapters only.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

const TEST: &str =
    "input::mapping::coverage_tests::t497_mapping_retries_verify_device_ownership_and_readback";
const SCREEN: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/screen-calls"
/bin/cat "$HOME/outputs"
"#;
const BUS: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/bus-calls"
case "$*" in
  *available*) printf 'b true\n';;
  *devicesSysNames*) /bin/cat "$HOME/devices";;
  *set-property*)
    [ ! -f "$HOME/refuse" ] || exit 17
    for value do :; done
    printf 's "%s"\n' "$value" > "$HOME/mapped";;
  *outputName*) /bin/cat "$HOME/readback";;
  *name*) /bin/cat "$HOME/name";;
  *) exit 42;;
esac
"#;

fn isolated() {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in [("kscreen-doctor", SCREEN), ("busctl", BUS)] {
        let path = dir.path().join(name);
        std::fs::write(&path, source).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env("BLENT_T497_MAPPING", "1")
        .env("HOME", dir.path())
        .env("PATH", dir.path())
        .env("XDG_RUNTIME_DIR", dir.path())
        .env("XDG_SESSION_TYPE", "wayland")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn outputs(root: &Path, inventory: &str) {
    std::fs::write(root.join("outputs"), inventory).unwrap();
}

fn physical(root: &Path) {
    outputs(
        root,
        r#"{"outputs":[{"id":1,"name":"fixture-physical","enabled":true,"primary":true}]}"#,
    );
}

async fn target_contract(root: &Path) {
    physical(root);
    assert_eq!(
        target_output(true, None, Duration::ZERO, None)
            .await
            .as_deref(),
        Some("fixture-physical")
    );
    outputs(root, r#"{"outputs":[]}"#);
    assert!(target_output(true, None, Duration::ZERO, None)
        .await
        .is_none());
    assert!(target_output(true, None, Duration::from_millis(1), None)
        .await
        .is_none());
    outputs(root, "invalid");
    assert!(target_output(true, None, Duration::ZERO, None)
        .await
        .is_none());
    assert!(target_output(false, Some(u32::MAX), Duration::ZERO, None)
        .await
        .is_none());
    connector_contract(root).await;
}

async fn connector_contract(root: &Path) {
    let names = [crate::vdisplay::EvdiConnector {
        name: "fixture-evdi".into(),
        card: 127,
        edid: Vec::new(),
        connected: true,
    }];
    assert_eq!(
        target_output(false, Some(127), Duration::ZERO, Some(&names))
            .await
            .as_deref(),
        Some("fixture-evdi")
    );
    outputs(root, r#"{"outputs":[]}"#);
    assert_eq!(
        target_output(false, Some(127), Duration::ZERO, Some(&names))
            .await
            .as_deref(),
        Some("fixture-evdi")
    );
    outputs(
        root,
        r#"{"outputs":[{"name":"fixture-evdi","enabled":true}]}"#,
    );
    assert_eq!(
        target_output(false, Some(127), Duration::ZERO, Some(&names))
            .await
            .as_deref(),
        Some("fixture-evdi")
    );
    assert!(
        target_output(false, Some(u32::MAX), Duration::ZERO, Some(&names))
            .await
            .is_none()
    );
    assert!(target_output(false, None, Duration::ZERO, Some(&[]))
        .await
        .is_none());
}

async fn device_contract(root: &Path, identity: &DeviceIdentity) {
    assert!(!map_kwin_device("event7", identity, "fixture-physical").await);
    std::fs::write(root.join("name"), "s \"Other input\"").unwrap();
    assert!(!map_kwin_device("event7", identity, "fixture-physical").await);
    assert!(!root.join("mapped").exists());
    std::fs::write(root.join("name"), format!("s \"{}\"", identity.pen)).unwrap();
    std::fs::write(root.join("refuse"), "").unwrap();
    assert!(!map_kwin_device("event7", identity, "fixture-physical").await);
    std::fs::remove_file(root.join("refuse")).unwrap();
    std::fs::write(root.join("readback"), "s \"wrong-output\"").unwrap();
    assert!(!map_kwin_device("event7", identity, "fixture-physical").await);
    std::fs::write(root.join("readback"), "s \"fixture-physical\"").unwrap();
    assert!(map_kwin_device("event7", identity, "fixture-physical").await);
    assert_eq!(
        std::fs::read_to_string(root.join("mapped")).unwrap(),
        "s \"fixture-physical\"\n"
    );
}

async fn mapping_contract(root: &Path, identity: &DeviceIdentity) {
    map_devices_to_output(true, identity, None, 0).await;
    physical(root);
    map_devices_to_output(true, identity, None, 1).await;
    assert!(crate::test_logging::text().contains("KWin did not answer"));
    std::fs::write(root.join("devices"), "as 1 \"event7\"").unwrap();
    map_devices_to_output(true, identity, None, 1).await;
    std::fs::write(root.join("devices"), "as 0").unwrap();
    map_devices_to_output(true, identity, None, 1).await;
    assert!(crate::test_logging::text().contains("did not appear in KWin"));
    outputs(root, "invalid");
    map_devices_to_output(true, identity, None, 1).await;
    map_devices_to_output(false, identity, Some(u32::MAX), 1).await;
    let logs = crate::test_logging::text();
    assert!(logs.contains("No physical output found"));
    assert!(logs.contains("No EVDI output found"));
}

async fn adapter_failures(root: &Path) {
    assert!(x11_query(
        "missing-t497-xinput",
        &[],
        "fixture-xinput",
        "fixture failure"
    )
    .await
    .is_none());
    assert!(x11_query(
        root.join("busctl").to_str().unwrap(),
        &["unknown"],
        "fixture-bus",
        "fixture command failed"
    )
    .await
    .is_none());
    let (mode, mut updates) = tokio::sync::watch::channel(false);
    super::super::settings::apply_tablet_mode(&mode, true, true);
    assert!(*updates.borrow_and_update());
    super::super::settings::apply_tablet_mode(&mode, false, true);
    assert!(!*updates.borrow_and_update());
    let logs = crate::test_logging::text();
    for message in [
        "needs fixture-xinput",
        "fixture command failed",
        "Tablet switched to pen-only mode",
        "Tablet switched to second-screen mode",
    ] {
        assert!(
            logs.contains(message),
            "T497 missing adapter diagnostic: {message}"
        );
    }
}

#[tokio::test]
async fn t497_mapping_retries_verify_device_ownership_and_readback() {
    if std::env::var_os("BLENT_T497_MAPPING").is_none() {
        isolated();
        return;
    }
    crate::test_logging::enable();
    let root = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    let identity = DeviceIdentity::for_instance(127);
    target_contract(&root).await;
    device_contract(&root, &identity).await;
    mapping_contract(&root, &identity).await;
    adapter_failures(&root).await;
}
