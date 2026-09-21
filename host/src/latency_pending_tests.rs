//! T568: delayed ACKs identify one output ordinal, not the latest encoded total.
use super::*;

#[test]
fn t568_older_ack_preserves_newer_pending_frames_across_sequence_wrap() {
    let tracker = LatencyTracker::new();
    let encoder = tracker.encoder_started("libx264", (640, 480, 60, 20000, 18));
    for sequence in [u32::MAX - 1, u32::MAX, 0, 1] {
        tracker.on_encoded_for(sequence, &encoder);
    }
    tracker.on_rendered(u32::MAX - 1, 100);
    assert_eq!(
        encoder.unrendered_output(),
        3,
        "T568: older ACK hid newer output"
    );
    assert_eq!(encoder.samples_after(0)[0].output, 1);
    tracker.on_rendered(1, 100);
    assert_eq!(encoder.unrendered_output(), 0);
    assert_eq!(encoder.samples_after(1)[0].output, 4);
    tracker.on_encoded_for(2, &encoder);
    tracker.on_rendered(u32::MAX, 100);
    assert_eq!(
        encoder.unrendered_output(),
        1,
        "T568: stale ACK changed progress"
    );
}

#[cfg(not(feature = "inproc-encoder"))]
#[test]
fn t568_unpublished_outputs_keep_their_place_in_encoder_progress() {
    let tracker = LatencyTracker::new();
    let encoder = tracker.encoder_started("libx264", (640, 480, 60, 20000, 18));
    tracker.on_encoder_output(&encoder);
    tracker.on_encoder_output(&encoder);
    tracker.on_encoded_for(10, &encoder);
    tracker.on_encoder_output(&encoder);
    tracker.on_encoded_for(11, &encoder);
    tracker.on_rendered(10, 100);
    assert_eq!(
        encoder.unrendered_output(),
        2,
        "T568: pending output was erased"
    );
    assert_eq!(encoder.samples_after(0)[0].output, 3);
    tracker.on_rendered(11, 100);
    assert_eq!(encoder.unrendered_output(), 0);
    assert_eq!(encoder.samples_after(1)[0].output, 5);
}

#[test]
fn t568_bounded_ack_order_and_invalid_sequence_corpus_preserves_pending_count() {
    for count in 1..=64u32 {
        let tracker = LatencyTracker::new();
        let encoder = tracker.encoder_started("libx264", (640, 480, 60, 20000, 18));
        let base = u32::MAX - count / 2;
        for offset in 0..count {
            tracker.on_encoded_for(base.wrapping_add(offset), &encoder);
        }
        let offset = (count.rotate_left(13) ^ 0x9e37_79b9) % count;
        tracker.on_rendered(base.wrapping_add(offset), 100);
        let pending = u64::from(count - offset - 1);
        assert_eq!(encoder.unrendered_output(), pending, "T568: count={count}");
        tracker.on_rendered(base.wrapping_sub(1), 100);
        tracker.on_rendered(base.wrapping_add(offset), 100);
        assert_eq!(encoder.unrendered_output(), pending);
        tracker.on_rendered(base.wrapping_add(count - 1), 100);
        assert_eq!(encoder.unrendered_output(), 0);
    }
}
