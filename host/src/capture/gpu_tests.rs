use super::*;
use crate::{desktop::Desktop, vdisplay::EvdiConnector};

fn connectors() -> Vec<EvdiConnector> {
    vec![EvdiConnector {
        name: "DVI-I-2".into(),
        card: 7,
        connected: true,
    }]
}

fn configuration() -> CaptureConfig {
    CaptureConfig {
        encoder: "h264_vaapi_baseline".into(),
        ..Default::default()
    }
}

#[test]
fn t575_gpu_adapter_requires_owned_supported_capture() {
    let mut config = configuration();
    let mut outputs = connectors();
    assert_eq!(
        supported_connector(&config, Some(7), &outputs, Desktop::X11).unwrap(),
        "/sys/class/drm/card7-DVI-I-2/edid"
    );
    for desktop in [Desktop::Other, Desktop::KdeWayland] {
        assert!(supported_connector(&config, Some(7), &outputs, desktop).is_err());
    }
    for encoder in ["libx264", "h264_vaapi", "hevc_vaapi", "auto", "unknown"] {
        config.encoder = encoder.into();
        assert!(supported_connector(&config, Some(7), &outputs, Desktop::X11).is_err());
    }
    config = configuration();
    config.adaptive_idle = true;
    assert!(supported_connector(&config, Some(7), &outputs, Desktop::X11).is_err());
    config.adaptive_idle = false;
    config.ten_bit = true;
    assert!(supported_connector(&config, Some(7), &outputs, Desktop::X11).is_err());
    config.ten_bit = false;
    for card in [None, Some(6)] {
        assert!(supported_connector(&config, card, &outputs, Desktop::X11).is_err());
    }
    outputs[0].connected = false;
    assert!(supported_connector(&config, Some(7), &outputs, Desktop::X11).is_err());
    outputs[0].connected = true;
    outputs.extend(connectors());
    assert!(supported_connector(&config, Some(7), &outputs, Desktop::X11).is_err());
}

#[tokio::test]
async fn t575_gpu_command_preserves_user_settings_without_shell_parsing() {
    let config = CaptureConfig {
        vaapi_device: "/native/device with spaces".into(),
        stream_scale: 2,
        fps: 30,
        quality: 18,
        bitrate: 60000,
        ..configuration()
    };
    let child = command(OsStr::new("/bin/echo"), &config, (640, 400), "DVI-I-2");
    let arguments: Vec<_> = child
        .as_std()
        .get_args()
        .map(|s| s.to_str().unwrap())
        .collect();
    assert_eq!(
        arguments,
        [
            "DVI-I-2",
            "/native/device with spaces",
            "640",
            "400",
            "2",
            "30",
            "18",
            "60000",
            "0",
            "desktop"
        ]
    );
    let mut adapter = Adapter::default();
    assert!(adapter.try_start(&config, (640, 400), Some(7)).is_none());
    adapter.failed = true;
    assert!(adapter.try_start(&config, (640, 400), Some(7)).is_none());
    let child = adapter
        .start_with(
            OsStr::new("/bin/echo"),
            &config,
            (640, 400),
            Some(7),
            &connectors(),
            Desktop::X11,
        )
        .unwrap();
    assert!(child.wait_with_output().await.unwrap().status.success());
    assert!(adapter
        .start_with(
            OsStr::new("/missing/blent-gpu-capture"),
            &config,
            (640, 400),
            Some(7),
            &connectors(),
            Desktop::X11
        )
        .is_err());
}

#[test]
fn t575_gpu_failure_falls_back_once_without_detaching_monitor() {
    let mut adapter = Adapter::default();
    assert!(!adapter.encoder_finished());
    adapter.active = true;
    assert!(adapter.encoder_finished());
    assert!(adapter.failed && !adapter.active);
    assert!(!adapter.encoder_finished());
    adapter.active = true;
    adapter.stopped();
    assert!(!adapter.active && adapter.failed);
    let changes = super::super::SessionChanges {
        settings_changed: false,
        mode_changed: false,
        display_dropped: false,
        fifo_reset: false,
        gpu_fallback: true,
    };
    assert!(changes.keep_helper());
    assert!(!changes.crashed());
    let environment = Adapter::from_environment();
    assert_eq!(
        environment.helper,
        std::env::var_os("BLENT_X11_GPU_HELPER").map(PathBuf::from)
    );
}

#[tokio::test]
async fn t575_successful_gpu_spawn_and_failed_retry_preserve_adapter_state() {
    let mut adapter = Adapter {
        helper: Some("/bin/echo".into()),
        ..Default::default()
    };
    let child = adapter
        .try_start_using(
            &configuration(),
            (1280, 800),
            Some(7),
            &connectors(),
            Desktop::X11,
        )
        .unwrap();
    assert!(adapter.active && !adapter.failed);
    assert!(child.wait_with_output().await.unwrap().status.success());
    adapter.stopped();
    let directory = tempfile::tempdir().unwrap();
    let missing_helper = directory.path().join("missing-blent-gpu");
    assert!(!missing_helper.exists());
    adapter.helper = Some(missing_helper);
    assert!(adapter
        .try_start_using(
            &configuration(),
            (1280, 800),
            Some(7),
            &connectors(),
            Desktop::X11
        )
        .is_none());
    assert!(adapter.failed && !adapter.active);
    adapter.helper = Some("/bin/echo".into());
    // T581: rejecting one retry must not clear the failure for a later attempt.
    for attempt in 1..=3 {
        assert!(
            adapter
                .try_start_using(
                    &configuration(),
                    (1280, 800),
                    Some(7),
                    &connectors(),
                    Desktop::X11
                )
                .is_none(),
            "T581: failed adapter spawned on retry {attempt}"
        );
        assert!(
            adapter.failed && !adapter.active,
            "T581: retry {attempt} changed the latched failure state"
        );
    }
}

#[tokio::test]
async fn t575_capture_supervisor_starts_gpu_with_owned_monitor_identity() {
    let mut manager = super::super::CaptureManager::new(CaptureConfig {
        width: 1280,
        height: 800,
        fps: 30,
        ..configuration()
    });
    manager.helper.card = Some(7);
    manager.encoder.gpu = Adapter {
        helper: Some("/bin/echo".into()),
        inspect: || (connectors(), Desktop::X11),
        ..Default::default()
    };
    assert_eq!(manager.start_session_encoder().await.unwrap(), (1280, 800));
    let child = manager.encoder.child.take().unwrap();
    let output = child.wait_with_output().await.unwrap();
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .starts_with("/sys/class/drm/card7-DVI-I-2/edid "));
    assert_eq!(manager.helper.card, Some(7));
    assert!(manager.encoder.gpu.encoder_finished());
}

#[tokio::test]
async fn t575_gpu_child_is_killed_when_its_owner_is_cancelled() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("gpu fixture");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let child = command(script.as_os_str(), &configuration(), (1280, 800), "DVI-I-2")
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    drop(child);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while std::path::Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("T575: cancelled adapter retained a native process");
}

#[tokio::test]
async fn t575_native_stock_ffmpeg_packets_use_the_existing_wire_contract() {
    use crate::{framed_annex_b::FramedAnnexB, media::Codec};
    // Six synthetic patterned frames from stock FFmpeg 6.1.6; no desktop pixels.
    let fixture = include_bytes!("../../tests/fixtures/t575-gpu.tee");
    let mut input = tokio::io::BufReader::new(fixture.as_slice());
    let mut parser = FramedAnnexB::new(Codec::H264, Default::default());
    let mut previous = -1;
    for sequence in 0..6 {
        let (bytes, packets) = parser.read_from(&mut input).await.unwrap();
        assert!(bytes > 0);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].seq, sequence);
        assert_eq!(packets[0].is_idr, sequence == 0);
        assert!(packets[0].codec_config.is_some());
        let timestamp = parser.timestamp_us().unwrap();
        assert!(timestamp > previous);
        previous = timestamp;
    }
    assert_eq!(parser.read_from(&mut input).await.unwrap().0, 0);
}
