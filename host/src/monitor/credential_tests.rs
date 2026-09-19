//! T444: route retirement and credential delivery with isolated fake devices.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn fixture(root: &std::path::Path) -> Monitor {
    let mut state = super::tests::fixture();
    let adb = root.join("adb");
    std::fs::write(
        &adb,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$0.actions"
if [ "$3" = shell ]; then cat >> "$0.commands"; fi
"#,
    )
    .unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    state.config.adb = adb.to_string_lossy().into_owned();
    state
}

async fn ready(state: &mut Monitor) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            state.prepare_primary();
            if state
                .current
                .as_ref()
                .is_some_and(|serial| state.ready.contains(serial))
            {
                break;
            }
            tokio::select! {
                (serial, result) = state.mutations.next() => state.mutation_ready(serial, result),
                result = state.retiring.join_next(), if !state.retiring.is_empty() => {
                    apply_event(state, Event::Retired(result)).await;
                }
            }
        }
    })
    .await
    .expect("T444: attachment did not become ready");
}

#[tokio::test]
async fn t444_replacement_removes_both_old_routes_before_slot_reuse() {
    let root = tempfile::tempdir().unwrap();
    let mut state = fixture(root.path());
    state.change_primary(&Some("USB-A".into()));
    ready(&mut state).await;
    state.change_primary(&Some("USB-B".into()));
    ready(&mut state).await;
    let actions = std::fs::read_to_string(root.path().join("adb.actions")).unwrap();
    for port in [8890, 8891] {
        let remove = actions.find(&format!("-s USB-A reverse --remove tcp:{port}"));
        let replace = actions.find(&format!("-s USB-B reverse tcp:{port}"));
        assert!(
            matches!((remove, replace), (Some(a), Some(b)) if a < b),
            "T444: slot reused before old route retirement: {actions}"
        );
    }
    state.stop().await;
}

fn authenticated(root: &std::path::Path) -> Monitor {
    let mut state = fixture(root);
    state.config.tablet = session::Spec {
        capture: Default::default(),
        ports: (0, 0),
        token: Some("a".repeat(64)),
        devices: (false, false, false),
    }
    .prepare(state.config.extra.mode_tx.clone())
    .tablet;
    let directory = root.join("tokens");
    std::fs::create_dir(&directory).unwrap();
    state.config.token_dir = Some(directory);
    state
}

#[tokio::test]
async fn t444_changed_physical_identity_on_same_serial_retires_credentials() {
    let root = tempfile::tempdir().unwrap();
    let mut state = authenticated(root.path());
    state.probe_ready("USB".into(), Some("physical-A".into()));
    state.change_primary(&Some("USB".into()));
    ready(&mut state).await;
    let old = state.config.tablet.token().unwrap();
    let lease = state.config.tablet.lease();
    state.probe_ready("USB".into(), Some("physical-B".into()));
    assert!(
        !lease.apply(|| ()),
        "T444: same serial retained a replaced identity's lease"
    );
    ready(&mut state).await;
    assert_ne!(state.config.tablet.token().unwrap(), old);
    state.stop().await;
}

#[tokio::test]
async fn t444_delivery_rotation_retry_and_known_route_migration_preserve_focus() {
    let root = tempfile::tempdir().unwrap();
    let mut state = authenticated(root.path());
    for serial in ["USB-A", "192.0.2.5:5555"] {
        state.identities.insert(serial.into(), "physical-A".into());
    }
    state.change_primary(&Some("USB-A".into()));
    ready(&mut state).await;
    let first = state.config.tablet.token().unwrap().unwrap();
    state.change_primary(&Some("192.0.2.5:5555".into()));
    ready(&mut state).await;
    assert_eq!(state.config.tablet.token().unwrap().unwrap(), first);
    state.change_primary(&Some("USB-B".into()));
    ready(&mut state).await;
    let second = state.config.tablet.token().unwrap().unwrap();
    assert_ne!(first, second);
    state.redeliver("USB-B".into());
    let (serial, result) = state.mutations.next().await;
    state.mutation_ready(serial, result);
    let commands = std::fs::read_to_string(root.path().join("adb.commands")).unwrap();
    let actions = std::fs::read_to_string(root.path().join("adb.actions")).unwrap();
    assert_eq!(commands.matches(&first).count(), 2);
    assert_eq!(commands.matches(&second).count(), 2);
    assert!(
        !commands.contains("am start"),
        "T444: manual/background attachment stole focus"
    );
    assert!(!actions.contains(&first));
    assert!(!actions.contains(&second));
    assert_eq!(
        std::fs::read_to_string(root.path().join("tokens/token")).unwrap(),
        second
    );
    state.stop().await;
}

async fn drained(state: &mut Monitor) {
    while !state.retiring.is_empty() {
        let result = state.retiring.join_next().await;
        apply_event(state, Event::Retired(result)).await;
    }
}

#[tokio::test]
async fn t444_failed_credential_delivery_retries_before_first_launch() {
    let root = tempfile::tempdir().unwrap();
    let mut state = authenticated(root.path());
    state.config.auto_launch = true;
    let adb = root.path().join("adb");
    let source = std::fs::read_to_string(&adb).unwrap();
    std::fs::write(&adb, format!("{source}\nif [ \"$3\" = shell ] && [ -f \"$0.fail-token\" ]; then exit 1; fi\nexit 0\n")).unwrap();
    let failure = root.path().join("adb.fail-token");
    std::fs::write(&failure, "fail").unwrap();
    state.change_primary(&Some("USB".into()));
    state.prepare_primary();
    let (serial, result) = state.mutations.next().await;
    state.mutation_ready(serial, result);
    assert!(!state.ready.contains("USB"));
    assert!(state.forwarding["USB"].next.is_some());
    let token = state.config.tablet.token().unwrap();
    assert!(!std::fs::read_to_string(root.path().join("adb.commands"))
        .unwrap()
        .contains("am start"));
    drained(&mut state).await;
    std::fs::remove_file(failure).unwrap();
    state.forwarding.clear();
    ready(&mut state).await;
    assert_eq!(state.config.tablet.token().unwrap(), token);
    let commands = std::fs::read_to_string(root.path().join("adb.commands")).unwrap();
    assert_eq!(commands.matches("am broadcast").count(), 2);
    assert_eq!(commands.matches("am start").count(), 1);
    state.stop().await;
}

#[tokio::test]
async fn t444_unwritable_token_publication_never_prepares_a_route() {
    let root = tempfile::tempdir().unwrap();
    let mut state = authenticated(root.path());
    state.config.token_dir = Some(root.path().join("missing"));
    state.change_primary(&Some("USB".into()));
    state.prepare_primary();
    let (serial, result) = state.mutations.next().await;
    state.mutation_ready(serial, result);
    assert!(!state.ready.contains("USB"));
    drained(&mut state).await;
    let actions = std::fs::read_to_string(root.path().join("adb.actions")).unwrap();
    assert!(
        !actions.contains("reverse tcp:"),
        "T444: route prepared despite publication error"
    );
    state.stop().await;
}

#[tokio::test]
async fn t444_inflight_route_is_cancelled_before_replacement() {
    let root = tempfile::tempdir().unwrap();
    let mut state = fixture(root.path());
    let adb = root.path().join("adb");
    std::fs::write(
        &adb,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$0.actions"
if [ "$2" = USB-A ] && [ "$3" = reverse ] && [ "$4" != --remove ]; then
    touch "$0.entered"
    sleep 10
    echo late-forward >> "$0.actions"
fi
if [ "$3" = shell ]; then cat >> "$0.commands"; fi
"#,
    )
    .unwrap();
    state.change_primary(&Some("USB-A".into()));
    state.prepare_primary();
    tokio::time::timeout(Duration::from_secs(1), async {
        while !root.path().join("adb.entered").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    state.change_primary(&Some("USB-B".into()));
    ready(&mut state).await;
    let actions = std::fs::read_to_string(root.path().join("adb.actions")).unwrap();
    assert!(!actions.contains("late-forward"));
    assert!(
        actions.find("USB-A reverse --remove tcp:8891").unwrap()
            < actions.find("USB-B reverse tcp:8890").unwrap()
    );
    assert!(!state.mutations.contains("USB-A"));
    state.stop().await;
}

#[tokio::test]
async fn t444_auth_retry_uses_current_key_with_bounded_background_delivery() {
    let root = tempfile::tempdir().unwrap();
    let mut state = authenticated(root.path());
    state.change_primary(&Some("USB".into()));
    ready(&mut state).await;
    for count in 1..=4 {
        state.last_relaunch = Instant::now() - state.relaunch_wait;
        apply_event(&mut state, Event::PrimaryToken).await;
        let (serial, result) = state.mutations.next().await;
        state.mutation_ready(serial, result);
        assert_eq!(state.relaunches, count);
        apply_event(&mut state, Event::PrimaryToken).await;
        assert!(
            !state.mutations.contains("USB"),
            "T444: retry ignored backoff"
        );
    }
    state.relaunch_wait = Duration::from_secs(600);
    state.last_relaunch = Instant::now() - state.relaunch_wait;
    state.primary_token();
    assert_eq!(state.relaunch_wait, Duration::from_secs(600));
    let (serial, result) = state.mutations.next().await;
    state.mutation_ready(serial, result);
    state.current = None;
    state.primary_token();
    let commands = std::fs::read_to_string(root.path().join("adb.commands")).unwrap();
    assert_eq!(commands.matches("am broadcast").count(), 6);
    assert!(!commands.contains("am start"));
    state.current = Some("USB".into());
    state.stop().await;
}

#[tokio::test]
async fn t444_failed_owned_job_retires_pending_route_and_rejects_late_completion() {
    let root = tempfile::tempdir().unwrap();
    let mut state = fixture(root.path());
    state.pending.insert(
        "USB".into(),
        Pending {
            instance: 0,
            session: None,
        },
    );
    state.mutations.schedule("USB".into(), async {
        panic!("injected device worker failure")
    });
    let (serial, result) = state.mutations.next().await;
    assert!(result.as_ref().is_err_and(|error| error.is_panic()));
    state.mutation_ready(serial, result);
    assert!(!state.can_prepare("USB"));
    drained(&mut state).await;
    assert!(state.can_prepare("USB"));
    state.mutation_ready(
        "USB".into(),
        Ok(Mutation::Prepared {
            ready: true,
            retry: Default::default(),
        }),
    );
    assert!(!state.ready.contains("USB"));
    state.stop().await;
}
