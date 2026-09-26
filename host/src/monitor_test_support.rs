//! T431: fixture adapters drive the production owner; no copied retry logic.
use super::*;

fn fixture(assigned: &[String], auto_launch: bool, token: Option<&str>, adb: &str) -> Monitor {
    let (tablet, extra, _stop) = crate::discovery_tests::monitor_inputs(2, (18000, 18001));
    let mut state = Monitor::new(Config {
        ports: (18000, 18001),
        auto_launch,
        tablet,
        token_dir: None,
        relaunch: Default::default(),
        extra,
        adb: adb.into(),
    });
    state.ready.extend(assigned.iter().cloned());
    for serial in assigned {
        let prepared = session::Spec {
            capture: Default::default(),
            ports: (0, 0),
            token: token.map(str::to_owned),
            devices: (false, false, false),
        }
        .prepare(state.config.extra.mode_tx.clone());
        state.attachments.insert(serial.clone(), prepared.tablet);
    }
    state
}

async fn finish(state: &mut Monitor) {
    while state
        .ready
        .iter()
        .any(|serial| state.mutations.contains(serial))
    {
        let (serial, result) = state.mutations.next().await;
        state.mutation_ready(serial, result);
    }
}

pub(crate) async fn deliver_extra_token(
    requested: &str,
    token: Option<&str>,
    policies: &mut HashMap<String, RelaunchBackoff>,
    now: Instant,
    adb: &str,
) {
    let assigned: Vec<_> = policies.keys().cloned().collect();
    let mut state = fixture(&assigned, true, token, adb);
    state.token_retries = std::mem::take(policies);
    state.extra_token(requested.into(), now);
    finish(&mut state).await;
    *policies = std::mem::take(&mut state.token_retries);
    state.stop().await;
}

pub(crate) async fn tick_assigned_apps(
    assigned: &[String],
    auto_launch: bool,
    token: Option<&str>,
    policies: &mut HashMap<String, RelaunchBackoff>,
    now: Instant,
    adb: &str,
) {
    let mut state = fixture(assigned, auto_launch, token, adb);
    state.token_retries = std::mem::take(policies);
    state.last_reconnect = now - Duration::from_secs(11);
    state.tick();
    finish(&mut state).await;
    *policies = std::mem::take(&mut state.token_retries);
    state.stop().await;
}

// T390 now exercises token delivery; T445 deliberately removes process relaunch.
pub(crate) async fn deliver_assigned_tokens(assigned: &[String], token: Option<&str>, adb: &str) {
    let mut state = fixture(assigned, false, token, adb);
    for serial in assigned {
        state.redeliver(serial.clone());
    }
    finish(&mut state).await;
    state.stop().await;
}

#[tokio::test]
async fn t431_inflight_mutation_does_not_consume_token_backoff() {
    let mut state = fixture(&["tablet".into()], true, None, "/missing-t431-adb");
    state
        .token_retries
        .insert("tablet".into(), RelaunchBackoff::default());
    state
        .mutations
        .schedule("tablet".into(), std::future::pending());
    state.extra_token("tablet".into(), Instant::now());
    assert!(state.token_retries["tablet"].next.is_none());
    state.stop().await;
}

#[tokio::test]
async fn t431_reassignment_rejects_late_recovery_and_cancels_work() {
    let mut state = fixture(&["OLD".into()], true, None, "/missing-t431-adb");
    state
        .token_retries
        .insert("OLD".into(), RelaunchBackoff::default());
    state
        .mutations
        .schedule("OLD".into(), std::future::pending());
    state.remove_assignment("OLD");
    assert!(
        !state.mutations.contains("OLD"),
        "T431: cancellation ownership was not transferred"
    );
    assert!(state.retiring_routes.contains_key("OLD"));
    let retired = tokio::time::timeout(Duration::from_millis(500), state.retiring.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    state.retired(retired);
    state.mutation_ready("OLD".into(), Ok(Mutation::Token));
    state.extra_token("OLD".into(), Instant::now());
    assert!(!state.ready.contains("OLD"));
    assert!(!state.token_retries.contains_key("OLD"));
    assert!(!state.mutations.contains("OLD"));
    state.stop().await;
}

#[tokio::test]
async fn t420_token_retry_does_not_launch_over_another_foreground_app() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let adb = root.path().join("adb");
    std::fs::write(
        &adb,
        r#"#!/bin/sh
if [ "$4" = pidof ]; then echo 123; exit 0; fi
printf '%s\n' "$*" >> "$0.args"
cat >> "$0.commands"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    let token = "a".repeat(64);
    let mut state = fixture(
        &["tablet".into()],
        true,
        Some(&token),
        adb.to_str().unwrap(),
    );
    state.redeliver("tablet".into());
    finish(&mut state).await;
    state.stop().await;
    let command = std::fs::read_to_string(adb.with_extension("commands")).unwrap();
    let arguments = std::fs::read_to_string(adb.with_extension("args")).unwrap();
    assert!(
        !command.contains("am start"),
        "T420: token recovery must not launch an Activity"
    );
    assert!(
        command.contains("am broadcast -n io.github.geraldo_netto.blent/com.blent.TokenReceiver")
    );
    assert!(command.contains(&token));
    assert!(
        !arguments.contains(&token),
        "T420: token leaked to adb argv"
    );
}
