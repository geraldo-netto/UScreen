//! T443: failed inventory commands do not mean every tablet disconnected.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn fake_inventory(root: &std::path::Path) -> PathBuf {
    let path = root.join("adb");
    std::fs::write(
        &path,
        r#"#!/bin/sh
if [ "$1" = devices ]; then
    case "$(cat "$0.mode")" in
        failure) echo 'error: daemon unavailable' >&2; exit 1;;
        timeout) exec sleep 30;;
        malformed) echo 'not an inventory'; exit 0;;
        bad-status) printf 'List of devices attached\nOLD\tdevice\n'; exit 1;;
        empty) printf 'List of devices attached\n'; exit 0;;
    esac
fi
if [ "$4" = getprop ]; then echo old-identity; fi
if [ "$4" = pm ]; then echo package:/base.apk; fi
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

async fn inventory_case(mode: &str) -> (bool, bool) {
    let root = tempfile::tempdir().unwrap();
    let adb = fake_inventory(root.path());
    std::fs::write(adb.with_extension("mode"), mode).unwrap();
    let mut state = super::tests::fixture();
    state.config.adb = adb.to_str().unwrap().into();
    state.config.extra.max_tablets = 1;
    state.current = Some("OLD".into());
    state.ready.insert("OLD".into());
    state.config.tablet.begin(Some("old-identity".into()));
    state.checked_adb = true;
    state
        .discovery
        .refresh(vec!["OLD".into()], &state.config.adb);
    let (serial, identity) = state.discovery.next().await.unwrap();
    state.probe_ready(serial, identity);
    let accepted = state.config.tablet.lease();
    state.queue_inventory(false);
    let result = tokio::time::timeout(Duration::from_secs(7), state.inventory.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    state.inventory_ready(result);
    state.reconcile().await;
    let outcome = (
        state.current.as_deref() == Some("OLD"),
        accepted.apply(|| {}),
    );
    state.stop().await;
    outcome
}

#[tokio::test]
async fn t443_failed_inventory_preserves_assignment_and_accepted_lease() {
    for mode in ["failure", "malformed", "timeout"] {
        assert_eq!(inventory_case(mode).await, (true, true), "T443: {mode}");
    }
}

#[tokio::test]
async fn t443_confirmed_empty_inventory_retires_assignment() {
    assert_eq!(inventory_case("empty").await, (false, false));
}

#[tokio::test]
async fn t443_failed_exit_never_publishes_plausible_partial_inventory() {
    let root = tempfile::tempdir().unwrap();
    let adb = fake_inventory(root.path());
    std::fs::write(adb.with_extension("mode"), "bad-status").unwrap();
    assert_eq!(
        crate::adb_inventory::query(adb.to_str().unwrap()).await,
        None
    );
    assert_eq!(crate::adb_inventory::query("/missing-t443-adb").await, None);
}
