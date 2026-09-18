use super::*;

#[test]
fn t465_idle_content_and_isolated_frames_do_not_indicate_failure() {
    let latency = LatencyTracker::new();
    let e = latency.encoder_started("libx264", (640, 480, 60, 20000, 18));
    let mut progress = Progress::default();
    let now = Instant::now();
    assert_eq!(progress.deadline(&e, now), None);
    latency.on_encoded_for(0, &e);
    assert_eq!(progress.deadline(&e, now + Duration::from_secs(90)), None);
    latency.on_rendered(0, 100);
    assert_eq!(progress.deadline(&e, now + Duration::from_secs(91)), None);
    assert_eq!(progress.deadline(&e, now + Duration::from_secs(180)), None);
}

#[test]
fn t465_retired_ack_cannot_hide_failed_replacement() {
    let latency = LatencyTracker::new();
    let format = (640, 480, 60, 20000, 18);
    let old = latency.encoder_started("libx264", format);
    latency.on_encoded_for(0, &old);
    let current = latency.encoder_started("libx264", format);
    let now = Instant::now();
    let mut progress = Progress::default();
    for seq in 1..5 {
        latency.on_encoded_for(seq, &current);
    }
    assert_eq!(progress.deadline(&current, now), Some(now + STALL_WINDOW));
    latency.on_rendered(0, 100);
    assert_eq!(
        progress.deadline(&current, now + STALL_WINDOW),
        Some(now + STALL_WINDOW)
    );
    latency.on_rendered(4, 100);
    assert_eq!(progress.deadline(&current, now + STALL_WINDOW), None);
}

#[test]
fn t465_repeated_encoder_restarts_without_output_do_not_reset_recovery_deadline() {
    let latency = LatencyTracker::new();
    let format = (640, 480, 60, 20000, 18);
    let old = latency.encoder_started("libx264", format);
    let mut progress = Progress::default();
    let now = Instant::now();
    drop(latency.encoder_activity(old.clone()));
    assert_eq!(progress.deadline(&old, now), Some(now + STALL_WINDOW));
    let replacement = latency.encoder_started("libx264", format);
    assert_eq!(
        progress.deadline(&replacement, now + Duration::from_secs(2)),
        Some(now + STALL_WINDOW)
    );
}

#[test]
fn t465_progress_before_monitor_subscription_and_decoder_recovery_are_observed() {
    let latency = LatencyTracker::new();
    let e = latency.encoder_started("libx264", (640, 480, 60, 20000, 18));
    for seq in 0..3 {
        latency.on_encoded_for(seq, &e);
        latency.on_rendered(seq, 100);
    }
    // These packets can arrive after verification but before monitoring subscribes.
    for seq in 3..7 {
        latency.on_encoded_for(seq, &e);
    }
    let mut progress = Progress::default();
    let now = Instant::now();
    assert_eq!(progress.deadline(&e, now), Some(now + STALL_WINDOW));
    latency.on_rendered(6, 100);
    assert_eq!(progress.deadline(&e, now + Duration::from_secs(5)), None);
    assert_eq!(progress.deadline(&e, now + Duration::from_secs(60)), None);
}
