//! T431: fixture adapters drive the production owner; no copied retry logic.
use super::*;

fn fixture(assigned: &[String], auto_launch: bool, token: Option<&str>, adb: &str) -> Monitor {
    let (tablet, extra, _stop) = crate::discovery_tests::monitor_inputs(2, (18000, 18001));
    let mut state = Monitor::new(Config {
        ports: (18000, 18001),
        auto_launch,
        tablet,
        token: token.map(str::to_owned),
        relaunch: Default::default(),
        extra,
        adb: adb.into(),
    });
    state.ready.extend(assigned.iter().cloned());
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
    state.recovery = std::mem::take(policies);
    state.extra_token(requested.into(), now);
    finish(&mut state).await;
    *policies = std::mem::take(&mut state.recovery);
    state.stop().await;
}

pub(crate) async fn recover_assigned_apps(
    assigned: &[String],
    auto_launch: bool,
    token: Option<&str>,
    policies: &mut HashMap<String, RelaunchBackoff>,
    now: Instant,
    adb: &str,
) {
    let mut state = fixture(assigned, auto_launch, token, adb);
    state.recovery = std::mem::take(policies);
    state.recover_apps(now);
    finish(&mut state).await;
    *policies = std::mem::take(&mut state.recovery);
    state.stop().await;
}

#[tokio::test]
async fn t431_inflight_mutation_does_not_consume_token_backoff() {
    let mut state = fixture(&["tablet".into()], true, None, "/missing-t431-adb");
    state
        .recovery
        .insert("tablet".into(), RelaunchBackoff::default());
    state
        .mutations
        .schedule("tablet".into(), std::future::pending());
    state.extra_token("tablet".into(), Instant::now());
    assert!(state.recovery["tablet"].next.is_none());
    state.stop().await;
}

#[tokio::test]
async fn t431_reassignment_rejects_late_recovery_and_cancels_work() {
    let mut state = fixture(&["OLD".into()], true, None, "/missing-t431-adb");
    state
        .recovery
        .insert("OLD".into(), RelaunchBackoff::default());
    state
        .mutations
        .schedule("OLD".into(), std::future::pending());
    state.remove_assignment("OLD");
    let (serial, result) = tokio::time::timeout(Duration::from_millis(500), state.mutations.next())
        .await
        .unwrap();
    assert!(result.as_ref().is_err_and(|error| error.is_cancelled()));
    state.mutation_ready(serial, result);
    state.mutation_ready(
        "OLD".into(),
        Ok(Mutation::Recovered(RelaunchBackoff::default())),
    );
    state.extra_token("OLD".into(), Instant::now());
    assert!(!state.ready.contains("OLD"));
    assert!(!state.recovery.contains_key("OLD"));
    assert!(!state.mutations.contains("OLD"));
    state.stop().await;
}
