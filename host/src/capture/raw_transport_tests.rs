use super::*;

#[test]
fn t418_encoder_change_reconfigures_automatic_raw_transport() {
    let manager = CaptureManager::new(CaptureConfig {
        encoder: "libx264".into(),
        ..Default::default()
    });
    let mut settings = manager_settings(&manager);
    assert!(!manager.helper_settings_changed(&settings));
    settings.quality += 1;
    assert!(!manager.helper_settings_changed(&settings));
    settings.encoder = "h264_nvenc".into();
    assert_eq!(
        manager.helper_settings_changed(&settings),
        cfg!(feature = "inproc-encoder"),
        "T418: Auto must change input adapters when leaving the measured software backend"
    );
}
fn manager_settings(manager: &CaptureManager) -> EncoderSettings {
    let c = &manager.config;
    EncoderSettings {
        encoder: c.encoder.clone(),
        fps: c.fps,
        bitrate: c.bitrate,
        width: c.width,
        height: c.height,
        quality: c.quality,
        width_mm: c.width_mm,
        height_mm: c.height_mm,
        stream_scale: c.stream_scale,
        geometry_ready: true,
        decoders: None,
        decoder_epoch: 0,
        selection: None,
    }
}
