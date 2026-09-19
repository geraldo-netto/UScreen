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
    assert_eq!(probes.calls.load(Ordering::SeqCst), 2);
    assert!(cache.load(cache::now(), &snapshot, &[row]).is_none());
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
    assert!(cached(
        &tx,
        &snapshot,
        &latency,
        Some(&cache),
        std::slice::from_ref(&row)
    )
    .await
    .is_none());
    assert!(cache.load(cache::now(), &snapshot, &[row]).is_none());
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
    assert!(cache
        .load(cache::now(), &setup().0, std::slice::from_ref(&row))
        .is_some());
    row.observation = None;
    cache.remember(&row).await;
    assert!(cache.load(cache::now(), &setup().0, &[row]).is_some());
}
