//! T705: route ownership across Linux preparation, repair and retirement.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn fixture(listing: &str) -> (tempfile::TempDir, Monitor, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let adb = root.path().join("adb");
    std::fs::write(adb.with_extension("listing"), listing).unwrap();
    std::fs::write(
        &adb,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$0.log"
[ "$3" = reverse ] || { cat >/dev/null; exit 0; }
case "$4" in
--list) [ ! -f "$0.offline" ] || exit 1; cat "$0.listing"; exit 0 ;;
--remove) remote=$5 ;;
--no-rebind) remote=$5; target=$6
    grep -q "^UsbFfs $remote " "$0.listing" && exit 1 ;;
*) remote=$4; target=$5 ;;
esac
[ "$remote" != tcp:8891 ] || [ ! -f "$0.fail-input" ] || exit 1
awk -v remote="$remote" '$2 != remote' "$0.listing" > "$0.next"
mv "$0.next" "$0.listing"
[ "$4" = --remove ] || printf 'UsbFfs %s %s\n' "$remote" "$target" >> "$0.listing"
exit 0
"#,
    )
    .unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut state = tests::fixture();
    state.config.adb = adb.to_str().unwrap().into();
    state.current = Some("USB".into());
    (root, state, adb)
}

async fn prepare(state: &mut Monitor) {
    state.prepare_primary();
    let (serial, result) = state.mutations.next().await;
    state.mutation_ready(serial, result);
}

fn listing(adb: &std::path::Path) -> String {
    std::fs::read_to_string(adb.with_extension("listing")).unwrap()
}

#[tokio::test]
async fn t705_conflicting_initial_routes_remain_foreign() {
    let foreign = "UsbFfs tcp:8891 tcp:2222\n";
    let (_root, mut state, adb) = fixture(foreign);
    prepare(&mut state).await;
    let ready = state.ready.contains("USB");
    state.stop().await;
    assert!(!ready, "T705: conflicting initial route was rebound");
    assert_eq!(listing(&adb), foreign);
}

#[tokio::test]
async fn t705_matching_external_and_replaced_routes_survive_retirement() {
    let external = "UsbFfs tcp:8890 tcp:18000\n";
    let (_root, mut state, adb) = fixture(external);
    prepare(&mut state).await;
    assert!(state.ready.contains("USB"));
    let replaced = format!("{external}UsbFfs tcp:8891 tcp:2222\n");
    std::fs::write(adb.with_extension("listing"), &replaced).unwrap();
    state.stop().await;
    assert_eq!(listing(&adb), replaced, "T705: retired external mappings");
}

#[tokio::test]
async fn t705_partial_preparation_retires_only_successfully_owned_routes() {
    let foreign = "UsbFfs tcp:9999 localabstract:external\n";
    let (_root, mut state, adb) = fixture(foreign);
    std::fs::write(adb.with_extension("fail-input"), "").unwrap();
    prepare(&mut state).await;
    assert!(!state.ready.contains("USB"));
    state.stop().await;
    assert_eq!(listing(&adb), foreign);
    let log = std::fs::read_to_string(adb.with_extension("log")).unwrap();
    assert!(log.contains("--remove tcp:8890"));
    assert!(
        !log.contains("--remove tcp:8891"),
        "T705: attempted unowned removal"
    );
}

#[tokio::test]
async fn t705_repaired_routes_are_owned_and_retired() {
    let external = "UsbFfs tcp:8890 tcp:18000\n";
    let (_root, mut state, adb) = fixture(external);
    prepare(&mut state).await;
    std::fs::write(adb.with_extension("listing"), "").unwrap();
    state.check_forwarding();
    let (serial, result) = state.mutations.next().await;
    state.mutation_ready(serial, result);
    assert!(listing(&adb).contains("tcp:8890 tcp:18000"));
    state.stop().await;
    assert_eq!(listing(&adb), "");
}

#[tokio::test]
async fn t705_offline_cleanup_releases_slot_and_revokes_the_old_lease() {
    let (_root, mut state, adb) = fixture("");
    state.config.tablet = session::Spec {
        capture: Default::default(),
        ports: (0, 0),
        token: Some("a".repeat(64)),
        devices: (false, false, false),
    }
    .prepare(state.config.extra.mode_tx.clone())
    .tablet;
    prepare(&mut state).await;
    let lease = state.config.tablet.lease();
    assert!(lease.apply(|| ()));
    std::fs::write(adb.with_extension("offline"), "").unwrap();
    state.change_primary(&None);
    assert!(
        !lease.apply(|| ()),
        "T705: stale client survived retirement"
    );
    let retired = tokio::time::timeout(Duration::from_secs(1), state.retiring.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    state.retired(retired);
    assert!(state.retiring_slots.is_empty());
    assert!(state.routes.is_empty());
    assert!(!std::fs::read_to_string(adb.with_extension("log"))
        .unwrap()
        .contains("--remove"));
    state.stop().await;
}

#[tokio::test]
async fn t705_invalid_route_owners_fail_before_native_commands() {
    let (_root, _state, adb) = fixture("");
    let command = adb.to_str().unwrap();
    for byte in 0..=31u8 {
        let serial = format!("bad{}serial", char::from(byte));
        assert!(RouteOwner::default()
            .prepare(&serial, (18000, 18001), command)
            .await
            .is_err());
    }
    for serial in [String::new(), "x".repeat(1025), "bad\u{7f}".into()] {
        assert!(RouteOwner::default()
            .prepare(&serial, (18000, 18001), command)
            .await
            .is_err());
    }
    for ports in [(0, 1), (1, 0), (1, 1), (65535, 65535)] {
        assert!(RouteOwner::default()
            .prepare("USB", ports, command)
            .await
            .is_err());
    }
    assert!(!adb.with_extension("log").exists());
}
