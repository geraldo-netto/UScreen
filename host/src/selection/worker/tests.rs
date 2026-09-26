use super::*;
use crate::media::DecoderCapabilities;

pub(super) fn settings() -> EncoderSettings {
    EncoderSettings {
        encoder: "auto".into(),
        fps: 60,
        bitrate: 20000,
        quality: 18,
        width: 640,
        height: 480,
        width_mm: 220,
        height_mm: 138,
        stream_scale: 1,
        geometry_ready: true,
        decoder_epoch: 1,
        selection: None,
        decoders: Some(DecoderCapabilities {
            protocol: 1,
            width: 640,
            height: 480,
            fps: 60,
            codecs: vec!["h264".into(), "vp9".into(), "av1".into()],
            hardware: vec!["h264".into(), "vp9".into()],
            ..Default::default()
        }),
    }
}
pub(super) fn candidate(name: &str, hardware: bool, fps: f64, p95: u64) -> Candidate {
    Candidate {
        cached: false,
        hardware,
        decoder: None,
        observation: None,
        measurement: Measurement {
            workers_requested: 0,
            workers_effective: None,
            encoder: name.into(),
            fps,
            p95_us: p95,
            first_us: 1,
            stream: None,
            quality_db: Some(40.0),
        },
    }
}

#[tokio::test(start_paused = true)]
async fn t484_old_decoder_acks_cannot_certify_a_new_decoder_only_trial() {
    let snapshot = settings();
    let (tx, mut updates) = watch::channel(snapshot.clone());
    let latency = LatencyTracker::new();
    let old = latency.encoder_started("libx264", Key::new(&snapshot).format);
    let mut trial = candidate("libx264", true, 100.0, 10);
    trial.decoder = Some(blent_config::negotiation::DecoderChoice {
        name: "vendor.avc".into(),
        stream: blent_config::negotiation::StreamProfile {
            codec: "h264".into(),
            format: blent_config::negotiation::Profile {
                profile: "baseline".into(),
                level: 31,
                depth: 8,
            },
        },
        low_latency: false,
        operating_rate: Some(120),
    });
    let requested = trial.decoder.clone();
    let worker = tokio::spawn({
        let tx = tx.clone();
        let latency = latency.clone();
        async move { supervise(&tx, &snapshot, &latency, vec![trial], None).await }
    });
    updates.changed().await.unwrap();
    for seq in 0..3 {
        latency.on_encoded_for(seq, &old);
        latency.on_rendered(seq, 100);
    }
    tokio::task::yield_now().await;
    let verified = tx.borrow().selection.as_ref().unwrap().verified;
    updates.borrow_and_update();
    let current = latency.encoder_started_with_decoder("libx264", old.format, requested);
    for seq in 3..6 {
        latency.on_encoded_for(seq, &current);
        latency.on_rendered_from(
            seq,
            100,
            current.decoder.as_ref().map(|d| d.receipt()).as_deref(),
        );
    }
    updates.changed().await.unwrap();
    let replacement_verified = tx.borrow().selection.as_ref().unwrap().verified;
    worker.abort();
    let _ = worker.await;
    assert!(
        !verified,
        "T484: previous decoder acknowledged the replacement's trial"
    );
    assert!(
        replacement_verified,
        "T484: matching replacement ACKs must verify"
    );
}

#[test]
fn t478_rich_selection_requires_actual_profile_and_conversion_intersection() {
    let mut settings = settings();
    let mut report: DecoderCapabilities = serde_json::from_str(include_str!(
        "../../../../testdata/decoder-capabilities-v2.json"
    ))
    .unwrap();
    report.scope = Some(settings.decoder_epoch.to_string());
    settings.decoders = Some(report);
    let mut measured = candidate("libx264", true, 100.0, 10).measurement;
    assert!(compatible_candidate(&settings, measured.clone(), false).is_none());
    measured.stream = Some(blent_config::negotiation::StreamProfile {
        codec: "h264".into(),
        format: blent_config::negotiation::Profile {
            profile: "constrained-baseline".into(),
            depth: 8,
            level: 31,
        },
    });
    let selected = compatible_candidate(&settings, measured.clone(), false).unwrap();
    assert_eq!(selected.decoder.as_ref().unwrap().name, "vendor.avc");
    assert!(selected.hardware);
    measured.stream.as_mut().unwrap().format.depth = 10;
    assert!(
        compatible_candidate(&settings, measured, true).is_none(),
        "T478: H.264 capture path cannot negotiate ten bit"
    );
}

#[test]
fn t434_ranking_uses_measured_capacity_then_decoder_class_and_tail_cadence() {
    let hardware = candidate("vp9_vaapi", true, 120.0, 8);
    let software = candidate("libaom-av1", false, 180.0, 5);
    let overloaded = candidate("h264_nvenc", true, 20.0, 50);
    let faster = candidate("libx264", true, 240.0, 3);
    assert_eq!(rank(&hardware, &software, 60), Ordering::Less);
    assert_eq!(rank(&software, &overloaded, 60), Ordering::Less);
    assert_eq!(rank(&faster, &hardware, 60), Ordering::Less);
}

#[test]
fn t434_old_peers_stale_formats_and_explicit_choices_do_not_select_candidates() {
    let good = settings();
    let key = Key::new(&good);
    let (tx, _rx) = watch::channel(good.clone());
    assert!(publish(&tx, &key, "libvpx-vp9", "fixture"));
    assert_eq!(tx.borrow().effective_encoder(), "libvpx-vp9");
    for bad in [
        EncoderSettings {
            decoders: None,
            ..good.clone()
        },
        EncoderSettings {
            fps: 30,
            ..good.clone()
        },
        EncoderSettings {
            decoder_epoch: 2,
            ..good.clone()
        },
        EncoderSettings {
            encoder: "libx264".into(),
            ..good.clone()
        },
    ] {
        tx.send_replace(bad);
        assert!(!publish(&tx, &key, "libaom-av1", "stale"));
        assert_eq!(tx.borrow().effective_encoder(), "libx264");
    }
    assert!(!eligible(
        &EncoderSettings {
            decoders: None,
            ..good
        },
        true
    ));
}

#[tokio::test]
async fn t434_failed_render_trials_fall_through_and_preserve_fallback() {
    let (tx, _rx) = watch::channel(settings());
    let key = Key::new(&tx.borrow());
    let candidates = vec![
        candidate("libaom-av1", false, 120.0, 4),
        candidate("libvpx-vp9", true, 120.0, 6),
    ];
    let mut attempts = Vec::new();
    choose(&tx, &key, "libx264", candidates, |name| {
        attempts.push(name.clone());
        std::future::ready(name == "libvpx-vp9")
    })
    .await;
    assert_eq!(attempts, ["libaom-av1", "libvpx-vp9"]);
    assert_eq!(tx.borrow().effective_encoder(), "libvpx-vp9");
    choose(
        &tx,
        &key,
        "libx264",
        vec![candidate("libaom-av1", false, 120.0, 4)],
        |_| std::future::ready(false),
    )
    .await;
    assert_eq!(tx.borrow().effective_encoder(), "libx264");
    choose(&tx, &key, "libx264", vec![], |_| std::future::ready(true)).await;
    assert_eq!(tx.borrow().effective_encoder(), "libx264");
}

#[tokio::test]
async fn t434_peer_change_cancels_pending_work_but_selection_updates_do_not() {
    let (tx, mut rx) = watch::channel(settings());
    let key = Key::new(&tx.borrow());
    let (_visible, mut display) = watch::channel(true);
    let (_stop, mut stop) = watch::channel(false);
    let work = async {
        assert!(publish(&tx, &key, "libx264", "measuring"));
        tokio::task::yield_now().await;
        assert!(key.matches(&rx_snapshot(&tx)));
        tx.send_modify(|s| {
            s.clear_decoders();
        });
        std::future::pending::<()>().await;
    };
    assert!(until_changed(&key, &mut rx, &mut display, &mut stop, work)
        .await
        .is_none());
    assert_eq!(tx.borrow().effective_encoder(), "libx264");
}
fn rx_snapshot(tx: &watch::Sender<EncoderSettings>) -> EncoderSettings {
    tx.borrow().clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t434_closed_display_watch_terminates_selector() {
    let (settings, _rx) = watch::channel(settings());
    let (display_tx, display) = watch::channel(true);
    drop(display_tx);
    let (stop_tx, stop) = watch::channel(false);
    let config = CaptureConfig {
        instance: u32::MAX,
        ..Default::default()
    };
    let attachment = crate::attachment::Attachment::new(settings.clone());
    let mut task = spawn(
        config,
        settings,
        display,
        stop,
        Default::default(),
        attachment,
    );
    let result = tokio::time::timeout(Duration::from_millis(200), &mut task).await;
    if result.is_err() {
        stop_tx.send(true).unwrap();
        task.await.unwrap();
    }
    assert!(
        result.is_ok(),
        "T434: closed display source restarted selection indefinitely"
    );
}

#[test]
fn t434_unverified_candidate_is_never_a_rollback_target() {
    let (tx, _rx) = watch::channel(settings());
    let key = Key::new(&tx.borrow());
    publish(&tx, &key, "libaom-av1", "trial");
    assert_eq!(fallback_encoder(&tx.borrow()), "libx264");
    tx.send_modify(|s| s.selection.as_mut().unwrap().verified = true);
    assert_eq!(fallback_encoder(&tx.borrow()), "libaom-av1");
    tx.send_modify(|s| s.clear_decoders());
    assert_eq!(fallback_encoder(&tx.borrow()), "libx264");
}

#[test]
fn t434_only_fresh_acks_from_matching_encoder_certify_a_trial() {
    let latency = LatencyTracker::new();
    let key = Key::new(&settings());
    let old = latency.encoder_started("libx264", key.format);
    for seq in 0..3 {
        latency.on_encoded_for(seq, &old);
    }
    let current = latency.encoder_started("libvpx-vp9", key.format);
    for seq in 0..3 {
        latency.on_rendered(seq, 100);
    }
    assert!(!matches_evidence(
        latency.encoder_evidence(),
        &key,
        "libvpx-vp9",
        None,
        None
    ));
    for seq in 3..6 {
        latency.on_encoded_for(seq, &current);
        latency.on_rendered(seq, 100);
    }
    assert!(matches_evidence(
        latency.encoder_evidence(),
        &key,
        "libvpx-vp9",
        None,
        None
    ));
    assert!(!matches_evidence(
        latency.encoder_evidence(),
        &key,
        "libvpx-vp9",
        Some((current.epoch, 3)),
        None
    ));
    assert!(!matches_evidence(
        latency.encoder_evidence(),
        &key,
        "libaom-av1",
        None,
        None
    ));
}

#[tokio::test(start_paused = true)]
async fn t465_verified_stream_failure_advances_without_control_reconnect() {
    let initial = settings();
    let key = Key::new(&initial);
    let (tx, mut rx) = watch::channel(initial.clone());
    let latency = LatencyTracker::new();
    let observed = latency.clone();
    let task = tokio::spawn(async move {
        supervise(
            &tx,
            &initial,
            &observed,
            vec![
                candidate("libvpx-vp9", true, 120.0, 4),
                candidate("libx264", true, 120.0, 6),
            ],
            None,
        )
        .await;
    });
    rx.wait_for(|s| s.effective_encoder() == "libvpx-vp9")
        .await
        .unwrap();
    let encoder = latency.encoder_started("libvpx-vp9", key.format);
    for _ in 0..3 {
        let seq = latency.next_sequence();
        latency.on_encoded_for(seq, &encoder);
        latency.on_rendered(seq, 100);
    }
    rx.wait_for(|s| s.selection.as_ref().is_some_and(|s| s.verified))
        .await
        .unwrap();
    for _ in 0..4 {
        latency.on_encoded_for(latency.next_sequence(), &encoder);
    }
    let recovered = tokio::time::timeout(
        Duration::from_secs(10),
        rx.wait_for(|s| s.effective_encoder() == "libx264"),
    )
    .await;
    task.abort();
    assert!(
        matches!(recovered, Ok(Ok(_))),
        "T465: verified candidate remained selected after sustained lost render progress"
    );
}

#[tokio::test(start_paused = true)]
async fn t465_exhaustion_keeps_fallback_without_retrying_failed_candidates() {
    let initial = settings();
    let key = Key::new(&initial);
    let (tx, mut rx) = watch::channel(initial.clone());
    let latency = LatencyTracker::new();
    let observed = latency.clone();
    let task = tokio::spawn(async move {
        supervise(
            &tx,
            &initial,
            &observed,
            vec![candidate("libvpx-vp9", true, 120.0, 4)],
            None,
        )
        .await;
    });
    rx.wait_for(|s| s.effective_encoder() == "libvpx-vp9")
        .await
        .unwrap();
    let encoder = latency.encoder_started("libvpx-vp9", key.format);
    for seq in 0..3 {
        latency.on_encoded_for(seq, &encoder);
        latency.on_rendered(seq, 100);
    }
    rx.wait_for(|s| s.selection.as_ref().is_some_and(|s| s.verified))
        .await
        .unwrap();
    drop(latency.encoder_activity(encoder));
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rx.borrow().effective_encoder(), "libx264");
    assert!(!rx.borrow().selection.as_ref().unwrap().verified);
    assert!(rx
        .borrow()
        .selection
        .as_ref()
        .unwrap()
        .reason
        .contains("failed or did not render"));
}

#[test]
fn t465_retired_encoder_cannot_certify_a_trial_after_late_acks() {
    let latency = LatencyTracker::new();
    let key = Key::new(&settings());
    let old = latency.encoder_started("libx264", key.format);
    for seq in 0..3 {
        latency.on_encoded_for(seq, &old);
    }
    drop(latency.encoder_activity(old));
    for seq in 0..3 {
        latency.on_rendered(seq, 100);
    }
    assert!(!matches_evidence(
        latency.encoder_evidence(),
        &key,
        "libx264",
        None,
        None
    ));
}

#[tokio::test(start_paused = true)]
async fn t612_previous_worker_receipts_cannot_verify_a_new_budget() {
    let tracker = LatencyTracker::new();
    let key = Key::new(&settings());
    let old = tracker.encoder_started_with_budget("libx264", key.format, None, 1, None);
    let task = tokio::spawn({
        let tracker = tracker.clone();
        let key = key.clone();
        async move { rendered(&tracker, &key, "libx264", None, 2).await }
    });
    tokio::task::yield_now().await;
    for _ in 0..4 {
        let seq = tracker.next_sequence();
        tracker.on_encoded_for(seq, &old);
        tracker.on_rendered(seq, 100);
    }
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    let current = tracker.encoder_started_with_budget("libx264", key.format, None, 2, None);
    for _ in 0..3 {
        let seq = tracker.next_sequence();
        tracker.on_encoded_for(seq, &current);
        tracker.on_rendered(seq, 100);
    }
    assert!(task.await.unwrap());
}

#[test]
fn t612_legacy_peer_keeps_one_worker_and_manual_budget_is_fixed() {
    let mut snapshot = settings();
    let mut base = CaptureConfig::default();
    assert_eq!(probe_budgets(&base, &snapshot, "libx264"), [1]);
    snapshot.decoders.as_mut().unwrap().protocol = 2;
    assert_eq!(probe_budgets(&base, &snapshot, "libx264"), [1, 2, 4]);
    base.encoder_workers = 7;
    assert_eq!(probe_budgets(&base, &snapshot, "libx264"), [7]);
    assert_eq!(probe_budgets(&base, &snapshot, "h264_vaapi"), [0]);
}
