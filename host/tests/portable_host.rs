//! T523: reusable protocol ownership must not require a native display backend.
use blent::{attachment::Attachment, latency::LatencyTracker, media::EncoderSettings};
use tokio::sync::watch;

#[tokio::test]
async fn t523_attachment_replacement_retires_shared_protocol_ownership() {
    let (settings, _) = watch::channel(settings());
    let attachment = Attachment::with_token(settings.clone(), Some("initial".into()));
    attachment.begin(Some("tablet-one".into()));
    let first_token = attachment.token().unwrap();
    let old = attachment.lease();
    old.apply(|| settings.send_modify(|value| value.geometry_ready = true));
    attachment.begin(Some("tablet-two".into()));
    assert!(!settings.borrow().geometry_ready);
    assert_ne!(attachment.token().unwrap(), first_token);
    assert!(!old.apply(|| panic!("T523: retired protocol owner was admitted")));
    let mut retired = old.retirement();
    tokio::time::timeout(std::time::Duration::from_secs(1), retired.retired())
        .await
        .unwrap();
}

#[test]
fn t523_shared_video_ownership_preserves_delayed_ack_progress() {
    let tracker = LatencyTracker::new();
    let evidence = tracker.encoder_started("libx264", (640, 480, 30, 20000, 18));
    let owner = tracker.encoder_activity(evidence.clone());
    for sequence in [u32::MAX, 0, 1] {
        tracker.on_encoded_for(sequence, &evidence);
    }
    tracker.on_rendered(u32::MAX, 100);
    assert_eq!(evidence.unrendered_output(), 2);
    drop(owner);
    assert!(!evidence.active());
    assert!(!blent_config::platform::capabilities().display || cfg!(target_os = "linux"));
}

fn settings() -> EncoderSettings {
    EncoderSettings {
        encoder: "libx264".into(),
        fps: 30,
        bitrate: 20000,
        width: 640,
        height: 480,
        quality: 18,
        width_mm: 310,
        height_mm: 194,
        stream_scale: 1,
        geometry_ready: false,
        decoders: None,
        decoder_epoch: 0,
        selection: None,
    }
}
