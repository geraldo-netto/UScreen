use super::super::cache::tests::{setup, store};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FakeProbes {
    row: Candidate,
    calls: AtomicUsize,
}
impl Probes for FakeProbes {
    async fn candidates(&self, _: &EncoderSettings) -> Vec<Candidate> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        vec![self.row.clone()]
    }
}
fn acknowledge(
    latency: &LatencyTracker,
    snapshot: &EncoderSettings,
    row: &Candidate,
) -> Arc<crate::latency::EncoderEvidence> {
    let evidence = latency.encoder_started_with_decoder(
        &row.measurement.encoder,
        Key::new(snapshot).format,
        row.decoder.clone(),
    );
    for seq in 0..3 {
        latency.on_encoded_for(seq, &evidence);
        latency.on_rendered_from(
            seq,
            1000,
            row.decoder.as_ref().map(|d| d.receipt()).as_deref(),
        );
    }
    evidence
}

#[tokio::test(start_paused = true)]
async fn t480_reused_profile_failure_releases_admission_and_restarts_measurements() {
    let (snapshot, row) = setup();
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(store(dir.path().join("cache")));
    cache.save(cache::now(), &row).unwrap();
    let probes = FakeProbes {
        row: row.clone(),
        calls: AtomicUsize::new(0),
    };
    let latency = LatencyTracker::new();
    let (tx, mut updates) = watch::channel(snapshot.clone());
    let work = optimize_with(&probes, &tx, &snapshot, &latency, Some(cache.clone()));
    let driver = async {
        drop(updates.wait_for(|s| s.selection.is_some()).await.unwrap());
        assert!(!tx.borrow().selection.as_ref().unwrap().verified);
        let evidence = acknowledge(&latency, &snapshot, &row);
        drop(
            updates
                .wait_for(|s| s.selection.as_ref().is_some_and(|s| s.verified))
                .await
                .unwrap(),
        );
        assert!(
            ADMISSION.try_acquire().is_ok(),
            "T480: healthy cached profile monopolized probes"
        );
        assert!(tx
            .borrow()
            .selection
            .as_ref()
            .unwrap()
            .reason
            .contains("historical profile"));
        for seq in 3..6 {
            latency.on_encoded_for(seq, &evidence);
        }
        drop(
            updates
                .wait_for(|s| {
                    s.selection
                        .as_ref()
                        .is_some_and(|s| s.reason.starts_with("Measuring"))
                })
                .await
                .unwrap(),
        );
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(work, driver);
    })
    .await
    .unwrap();
    assert_eq!(probes.calls.load(Ordering::SeqCst), 1); // T714: probes only after failure.
    assert!(cache.load(cache::now(), &snapshot).is_none());
    assert_eq!(tx.borrow().effective_encoder(), "libx264");
    assert!(!tx.borrow().selection.as_ref().unwrap().verified);
}

#[tokio::test(start_paused = true)]
async fn t480_failed_fresh_render_check_discards_cache_and_preserves_fallback() {
    let (snapshot, row) = setup();
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("cache"));
    cache.save(cache::now(), &row).unwrap();
    let (tx, _) = watch::channel(snapshot.clone());
    let latency = LatencyTracker::new();
    assert!(cached(&tx, &snapshot, &latency, Some(&cache))
        .await
        .is_none());
    assert!(cache.load(cache::now(), &snapshot).is_none());
    assert!(!tx.borrow().selection.as_ref().unwrap().verified);
}

#[tokio::test(start_paused = true)]
async fn t480_interaction_cancels_calibration_without_claiming_success() {
    let (snapshot, row) = setup();
    let (tx, _) = watch::channel(snapshot.clone());
    let probes = FakeProbes {
        row,
        calls: AtomicUsize::new(0),
    };
    let latency = LatencyTracker::new();
    let work = optimize_with(&probes, &tx, &snapshot, &latency, None);
    let driver = async {
        tokio::task::yield_now().await;
        latency.note_interaction();
    };
    tokio::join!(work, driver);
    assert!(tx
        .borrow()
        .selection
        .as_ref()
        .unwrap()
        .reason
        .starts_with("Calibration interrupted"));
}

#[tokio::test(start_paused = true)]
async fn t480_legacy_peers_use_uncached_host_path_and_optional_save_failures_do_not_escape() {
    let mut snapshot = super::super::tests::settings();
    snapshot.decoders.as_mut().unwrap().codecs.clear();
    let (tx, _) = watch::channel(snapshot.clone());
    let attachment = Attachment::new(tx.clone());
    optimize(
        &CaptureConfig::default(),
        &tx,
        &snapshot,
        &LatencyTracker::new(),
        &attachment,
    )
    .await;
    assert!(tx
        .borrow()
        .selection
        .as_ref()
        .unwrap()
        .reason
        .contains("Preserved fallback"));
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(store(dir.path().join("cache")));
    let (_, mut row) = setup();
    cache.remember(&row).await;
    assert!(cache.load(cache::now(), &setup().0).is_some());
    row.observation = None;
    cache.remember(&row).await;
    assert!(cache.load(cache::now(), &setup().0).is_some());
}

#[tokio::test(start_paused = true)]
async fn t714_tuned_start_verifies_rendering_without_probe_matrix() {
    let (snapshot, row) = setup();
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("cache"));
    cache.save(cache::now(), &row).unwrap();
    let probes = FakeProbes {
        row: row.clone(),
        calls: AtomicUsize::new(0),
    };
    let latency = LatencyTracker::new();
    let (tx, mut updates) = watch::channel(snapshot.clone());
    let work = prepare(&probes, &tx, &snapshot, &latency, Some(&cache));
    let driver = async {
        drop(updates.wait_for(|s| s.selection.is_some()).await.unwrap());
        acknowledge(&latency, &snapshot, &row)
    };
    let (result, _evidence) = tokio::join!(work, driver);
    assert!(matches!(result, Some(Prepared::Cached(_))));
    assert_eq!(
        probes.calls.load(Ordering::SeqCst),
        0,
        "T714: tuned startup must skip the expensive probe matrix"
    );
}

#[tokio::test(start_paused = true)]
async fn t714_normal_retirement_preserves_tuned_profile_for_reconnect() {
    let (snapshot, row) = setup();
    let (tx, _) = watch::channel(snapshot.clone());
    let attachment = Attachment::new(tx);
    attachment.begin_with_transport(
        Some("device:T714-tablet".into()),
        Some(blent_config::adb::Transport::Usb),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache");
    let cache = super::super::cache::tests::attached_store(path.clone(), attachment.lease());
    cache.save(cache::now(), &row).unwrap();
    attachment.begin_with_transport(
        Some("device:T714-tablet".into()),
        Some(blent_config::adb::Transport::Usb),
    );
    watch_cached(&cache, &LatencyTracker::new(), &Key::new(&snapshot), &row).await;
    assert!(
        store(path).load(cache::now(), &snapshot).is_some(),
        "T714: normal attachment retirement erased durable tuning"
    );
}

#[tokio::test(start_paused = true)]
async fn t714_pending_and_terminal_states_follow_trial_outcomes() {
    let (snapshot, row) = setup();
    let key = Key::new(&snapshot);
    let (tx, _) = watch::channel(snapshot);
    assert!(tx.borrow().calibrating());
    let result = choose(&tx, &key, "libx264", vec![row.clone()], |_| {
        assert!(tx.borrow().calibrating());
        std::future::ready(true)
    })
    .await;
    assert!(result.is_some());
    assert!(!tx.borrow().calibrating());
    choose(&tx, &key, "libx264", vec![row], |_| {
        std::future::ready(false)
    })
    .await;
    assert!(!tx.borrow().calibrating());
    assert!(!tx.borrow().selection.as_ref().unwrap().verified);
    publish(&tx, &key, "libx264", "Cancelled trial; restored fallback");
    assert!(!tx.borrow().calibrating());
    tx.send_modify(|s| s.clear_decoders());
    assert!(tx.borrow().calibrating());
    assert!(!publish(&tx, &key, "libx264", "retired"));
    assert!(tx.borrow().calibrating());
    tx.send_modify(|s| s.encoder = "libx264".into());
    assert!(!tx.borrow().calibrating());
}
