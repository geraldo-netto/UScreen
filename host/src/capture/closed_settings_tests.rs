use super::*;

// T437: a closed settings source is terminal even when all other watches live.
// Two workers let the deadline/shutdown task run if the capture worker spins.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t437_closed_settings_stops_idle_capture() {
    let mut manager = CaptureManager::new(CaptureConfig {
        helper_path: "/nonexistent/blent-t437-helper".into(),
        instance: u32::MAX - 437,
        ..Default::default()
    });
    manager.helper.card = Some(u32::MAX);
    let (settings_tx, settings) = watch::channel(EncoderSettings {
        geometry_ready: false,
        encoder: "libx264".into(),
        fps: 60,
        bitrate: 20000,
        width: 1280,
        height: 800,
        quality: 18,
        width_mm: 220,
        height_mm: 138,
        stream_scale: 1,
        decoders: None,
        decoder_epoch: 0,
        selection: None,
    });
    drop(settings_tx);
    let (_display_tx, display) = watch::channel(false);
    let (stop_tx, stop) = watch::channel(false);
    let (video, _receiver) = crate::video_queue::channel(8, Default::default());
    let mut task =
        tokio::spawn(async move { manager.stream_frames(video, settings, display, stop).await });
    let result = tokio::time::timeout(std::time::Duration::from_millis(300), &mut task).await;
    if result.is_err() {
        stop_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    assert!(
        result.is_ok(),
        "T437: closed settings spun instead of terminating capture"
    );
    result.unwrap().unwrap().unwrap();
}
