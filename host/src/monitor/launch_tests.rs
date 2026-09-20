//! T445: attachment launch is independent of route repair and process recovery.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn adb(root: &std::path::Path) -> PathBuf {
    let path = root.join("adb");
    std::fs::write(
        &path,
        r#"#!/bin/sh
if [ "$1" = version ]; then exit 0; fi
if [ "$1" = devices ]; then printf 'List of devices attached\n'; exit 0; fi
if [ "$4" = getprop ]; then echo same-tablet; exit 0; fi
if [ "$4" = pm ]; then echo package:/base.apk; exit 0; fi
if [ "$3" = reverse ]; then
    if [ -f "$0.fail" ]; then exit 1; fi
    exit 0
fi
if [ "$4" = pidof ]; then exit 1; fi
cat >> "$0.commands"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn launches(adb: &std::path::Path) -> usize {
    std::fs::read_to_string(adb.with_extension("commands"))
        .unwrap_or_default()
        .matches("am start")
        .count()
}

async fn complete(state: &mut Monitor, serial: &str) {
    loop {
        if !state.retiring.is_empty() {
            let result = tokio::time::timeout(Duration::from_secs(2), state.retiring.join_next())
                .await
                .unwrap();
            apply_event(state, Event::Retired(result)).await;
            state.prepare_primary();
        } else if state.mutations.contains(serial) {
            let (serial, result) =
                tokio::time::timeout(Duration::from_secs(2), state.mutations.next())
                    .await
                    .unwrap();
            state.mutation_ready(serial, result);
        } else {
            break;
        }
    }
}

async fn inventory(state: &mut Monitor, serial: &str) {
    state.inventory_ready(Inventory {
        devices: Some(vec![serial.into()]),
        synthetic: vec![],
        reconnected: None,
    });
    assert!(!state.discovery.initial_probes_finished());
    state.reconcile().await;
    let (probed, identity) = tokio::time::timeout(Duration::from_secs(2), state.discovery.next())
        .await
        .unwrap()
        .unwrap();
    state.probe_ready(probed, identity);
    assert!(state.discovery.initial_probes_finished());
    state.reconcile().await;
    complete(state, serial).await;
}

#[tokio::test]
async fn t445_new_route_probe_preserves_launch_history_until_identity_is_known() {
    let root = tempfile::tempdir().unwrap();
    let adb = adb(root.path());
    let mut state = super::tests::fixture();
    state.config.adb = adb.to_str().unwrap().into();
    state.config.auto_launch = true;
    inventory(&mut state, "USB").await;
    assert_eq!(launches(&adb), 1);
    inventory(&mut state, "192.0.2.8:5555").await;
    assert_eq!(
        launches(&adb),
        1,
        "T445: async identity probe forgot same-device launch"
    );
    state.inventory_ready(Inventory {
        devices: Some(vec![]),
        synthetic: vec![],
        reconnected: None,
    });
    state.reconcile().await;
    inventory(&mut state, "USB").await;
    assert_eq!(launches(&adb), 2);
    state.stop().await;
}

#[tokio::test]
async fn t445_same_attachment_never_reopens_after_route_repair_or_process_loss() {
    let root = tempfile::tempdir().unwrap();
    let adb = adb(root.path());
    let mut state = super::tests::fixture();
    state.config.auto_launch = true;
    state.config.adb = adb.to_str().unwrap().into();
    for serial in ["USB", "192.0.2.8:5555"] {
        state.identities.insert(serial.into(), "same-tablet".into());
        state.change_primary(&Some(serial.into()));
        state.prepare_primary();
        complete(&mut state, serial).await;
        assert_eq!(
            launches(&adb),
            1,
            "T445: transport migration reopened the Activity"
        );
        state.remove_assignment(serial);
        state.prepare_primary();
        complete(&mut state, serial).await;
        assert_eq!(
            launches(&adb),
            1,
            "T445: route revalidation reopened the Activity"
        );
        state.last_reconnect = Instant::now() - Duration::from_secs(11);
        state.tick();
        complete(&mut state, serial).await;
        assert_eq!(
            launches(&adb),
            1,
            "T445: process loss stole foreground focus"
        );
    }
    launch_app_using("192.0.2.8:5555", None, adb.to_str().unwrap()).await;
    assert_eq!(
        launches(&adb),
        2,
        "T445: explicit reopening must remain available"
    );
    state.inventory_ready(Inventory {
        devices: Some(vec![]),
        synthetic: vec![],
        reconnected: None,
    });
    state.reconcile().await;
    state.change_primary(&Some("USB".into()));
    state.prepare_primary();
    complete(&mut state, "USB").await;
    assert_eq!(
        launches(&adb),
        3,
        "T445: a fresh attachment must get a new launch"
    );
    state.stop().await;
}

#[tokio::test]
async fn t445_forwarding_retry_keeps_the_first_launch_and_disabled_stays_manual() {
    let root = tempfile::tempdir().unwrap();
    let adb = adb(root.path());
    let mut state = super::tests::fixture();
    state.config.adb = adb.to_str().unwrap().into();
    state.config.auto_launch = true;
    state.change_primary(&Some("USB".into()));
    std::fs::write(adb.with_extension("fail"), "failed route").unwrap();
    state.prepare_primary();
    complete(&mut state, "USB").await;
    assert_eq!(launches(&adb), 0);
    std::fs::remove_file(adb.with_extension("fail")).unwrap();
    state.forwarding.clear(); // T144 separately verifies the actual retry deadlines.
    state.prepare_primary();
    complete(&mut state, "USB").await;
    assert_eq!(launches(&adb), 1);
    state.config.auto_launch = false;
    state.change_primary(&Some("OTHER".into()));
    state.prepare_primary();
    complete(&mut state, "OTHER").await;
    assert_eq!(launches(&adb), 1);
    state.stop().await;
}

#[tokio::test]
async fn t445_synthetic_attachment_never_launches_a_real_android_activity() {
    if !is_fake_serial("fake:t445") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "monitor::launch_tests::t445_synthetic_attachment_never_launches_a_real_android_activity", "--nocapture"])
            .env("USCREEN_FAKE_TABLET", "fake:t445").output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let adb = adb(root.path());
    let mut state = super::tests::fixture();
    state.config.adb = adb.to_str().unwrap().into();
    state.config.auto_launch = true;
    state.change_primary(&Some("fake:t445".into()));
    state.prepare_primary();
    complete(&mut state, "fake:t445").await;
    assert_eq!(launches(&adb), 0);
    state.stop().await;
}

fn t557_adb(root: &std::path::Path) -> PathBuf {
    let path = adb(root);
    let original = std::fs::read_to_string(&path).unwrap();
    let reverse = r#"if [ "$3" = reverse ]; then
    if [ "$4" = --list ]; then cat "$0.maps" 2>/dev/null; exit 0; fi
    if [ "$4" = --remove ]; then exit 0; fi
    if [ "$4" = --no-rebind ]; then shift; fi
    printf 'UsbFfs %s %s\n' "$4" "$5" >> "$0.maps"
    exit 0
fi"#;
    let begin = original.find("if [ \"$3\" = reverse ]; then").unwrap();
    let end = begin + original[begin..].find("\nfi").unwrap() + 3;
    let script = format!("{}{}{}", &original[..begin], reverse, &original[end..]);
    std::fs::write(&path, script).unwrap();
    path
}

#[tokio::test]
async fn t557_ready_attachment_repairs_lost_mappings_without_reopening_android() {
    let root = tempfile::tempdir().unwrap();
    let adb = t557_adb(root.path());
    let mut state = super::tests::fixture();
    state.config.adb = adb.to_str().unwrap().into();
    state.config.auto_launch = true;
    state.config.ports = (9010, 9011);
    state.change_primary(&Some("USB".into()));
    state.prepare_primary();
    complete(&mut state, "USB").await;
    assert!(state.ready.contains("USB"));
    assert_eq!(launches(&adb), 1);
    std::fs::remove_file(adb.with_extension("maps")).unwrap();
    state.last_reconnect = Instant::now() - Duration::from_secs(11);
    state.tick();
    complete(&mut state, "USB").await;
    let mappings = std::fs::read_to_string(adb.with_extension("maps")).unwrap_or_default();
    assert!(
        mappings.contains("tcp:8890 tcp:9010") && mappings.contains("tcp:8891 tcp:9011"),
        "T557: ready attachment left both ADB reverse mappings missing: {mappings:?}"
    );
    assert_eq!(launches(&adb), 1, "T557: route repair reopened Android");
    assert!(state.ready.contains("USB"));
    state.stop().await;
}
