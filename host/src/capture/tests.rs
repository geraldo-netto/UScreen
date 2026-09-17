use super::*;
use crate::media::Codec;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
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
    }
}
#[tokio::test]
async fn t347_helper_receives_the_original_edid_filename() {
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let helper = root.path().join("read-edid");
    std::fs::write(&helper, "#!/bin/sh\nexec cat -- \"$2\"\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let expected = crate::edid::make_edid(1280, 800, 60);
    for name in [
        b"custom edid.bin".as_slice(),
        b"custom \xff edid.bin".as_slice(),
    ] {
        let path = root
            .path()
            .join(std::ffi::OsString::from_vec(name.to_vec()));
        std::fs::write(&path, &expected).unwrap();
        let mut manager = test_manager();
        manager.config.helper_path = helper.clone();
        manager.config.edid_path = Some(path);
        let output = manager
            .helper
            .command(&manager.config, Path::new("unused-test-fifo"))
            .unwrap()
            .stderr(Stdio::piped())
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "T347: helper could not read the original EDID: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected);
    }
}
#[tokio::test]
async fn t223_initial_attach_waits_for_geometry_on_each_daemon_start() {
    for _ in 0..2 {
        t223_initial_attach().await;
    }
}
async fn t223_initial_attach() {
    assert_initial_geometry_gate(
        test_manager(),
        |s| {
            s.width = 1280;
            s.height = 800;
            s.width_mm = 220;
            s.height_mm = 138;
            s.geometry_ready = true;
        },
        (1280, 800, 220, 138),
    )
    .await;
}
#[tokio::test]
async fn t275_manual_geometry_opens_capture_gate_for_a_larger_native_panel() {
    let mut manager = test_manager();
    manager.config.width = 1920;
    manager.config.height = 1080;
    assert_initial_geometry_gate(
        manager,
        |current| {
            if let Some(next) =
                crate::input::negotiated_geometry(current, (5120, 3200), (220, 138), false)
            {
                *current = next;
            }
        },
        (1920, 1080, 220, 138),
    )
    .await;
}
async fn assert_initial_geometry_gate(
    mut manager: CaptureManager,
    negotiate: impl FnOnce(&mut EncoderSettings),
    expected: (u32, u32, u32, u32),
) {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let helper = root.path().join("helper");
    std::fs::write(&helper, "#!/bin/sh\necho attach >> \"$0.log\"\nsleep 0.1\necho 'EVDI_CONNECTED card4294967295'\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    manager.config.helper_path = helper.to_str().unwrap().into();
    manager.config.edid_path = Some(root.path().join("test.edid"));
    let mut initial = manager_settings(&manager);
    initial.geometry_ready = false;
    let (settings, settings_rx) = watch::channel(initial);
    let (_display, display) = watch::channel(true);
    let (shutdown, stop) = watch::channel(false);
    let (video, _) = broadcast::channel(8);
    let task = tokio::spawn(async move {
        manager
            .stream_frames(video, settings_rx, display, stop)
            .await
            .unwrap();
        manager.config.clone()
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let premature = helper.with_extension("log").exists();
    settings.send_modify(negotiate);
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    shutdown.send(true).unwrap();
    let config = task.await.unwrap();
    assert!(
        !premature,
        "T223: attached default mode before tablet metadata"
    );
    assert_eq!(
        (
            config.width,
            config.height,
            config.width_mm,
            config.height_mm
        ),
        expected
    );
    assert_eq!(
        std::fs::read_to_string(helper.with_extension("log")).unwrap(),
        "attach\n"
    );
}
#[tokio::test]
async fn t223_setup_observes_settings_without_hotplug_for_encoder_only_changes() {
    let manager = test_manager();
    let (tx, mut settings) = watch::channel(manager_settings(&manager));
    let (_display, mut display) = watch::channel(true);
    let (_shutdown, mut shutdown) = watch::channel(false);
    let update = async {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        tx.send_modify(|s| s.bitrate += 1000);
    };
    let (result, _) = tokio::join!(
        CaptureManager::while_settings_current(
            &mut settings,
            &mut display,
            &mut shutdown,
            tokio::time::sleep(std::time::Duration::from_millis(40)),
            true
        ),
        update
    );
    assert!(
        result.is_some(),
        "T223: encoder settings must not cancel the attaching helper"
    );
    assert!(
        settings.has_changed().unwrap(),
        "T223: apply new encoder settings before encoding"
    );
    settings.borrow_and_update();
    let update = async {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        tx.send_modify(|s| s.width = 1280);
    };
    let (result, _) = tokio::join!(
        CaptureManager::while_settings_current(
            &mut settings,
            &mut display,
            &mut shutdown,
            std::future::pending::<()>(),
            true
        ),
        update
    );
    assert!(
        result.is_none(),
        "T223: geometry change must interrupt stale helper setup"
    );
}
#[tokio::test]
async fn t091_shutdown_interrupts_real_capture_retry() {
    let mut manager = test_manager();
    let root = tempfile::tempdir().unwrap();
    manager.config.edid_path = Some(root.path().join("test.edid"));
    let (_settings, settings) = watch::channel(manager_settings(&manager));
    let (_display, display) = watch::channel(true);
    let (shutdown, stop) = watch::channel(false);
    let (video, _) = broadcast::channel(8);
    let mut task =
        tokio::spawn(async move { manager.stream_frames(video, settings, display, stop).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    shutdown.send(true).unwrap();
    let stopped = tokio::time::timeout(std::time::Duration::from_millis(300), &mut task).await;
    task.abort();
    assert!(matches!(stopped, Ok(Ok(Ok(())))), "retry ignores shutdown");
}
#[tokio::test]
async fn t091_setup_stages_cancel_on_shutdown_or_detach() {
    for shutdown in [false, true] {
        for stage in 0..3 {
            assert_setup_stage_cancels(stage, shutdown).await;
        }
    }
}
async fn assert_setup_stage_cancels(stage: u8, shutdown: bool) {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let helper = root.path().join("helper");
    std::fs::write(&helper, "#!/bin/sh\necho $$ > \"$0.pid\"\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut manager = test_manager();
    manager.config.helper_path = helper.clone();
    manager.config.edid_path = Some(root.path().join("test.edid"));
    let (display_tx, mut display) = watch::channel(true);
    let (shutdown_tx, mut stop) = watch::channel(false);
    let operation = async {
        match stage {
            0 => {
                let _ = manager.start_helper().await;
            }
            1 => manager.helper.wait_stream_size(&manager.config).await,
            _ => tokio::time::sleep(std::time::Duration::from_secs(30)).await,
        }
    };
    let cancel = async {
        if stage == 0 {
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while !helper.with_extension("pid").exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        if shutdown {
            shutdown_tx.send(true).unwrap();
        } else {
            display_tx.send(false).unwrap();
        }
    };
    let result = tokio::time::timeout(std::time::Duration::from_millis(1200), async {
        tokio::join!(
            CaptureManager::while_active(&mut display, &mut stop, operation),
            cancel
        )
        .0
    })
    .await;
    assert!(
        matches!(result, Ok(None)),
        "stage {stage} ignored cancellation"
    );
    if stage == 0 {
        let pid = std::fs::read_to_string(helper.with_extension("pid")).unwrap();
        tokio::time::timeout(std::time::Duration::from_millis(300), async {
            while std::path::Path::new(&format!("/proc/{}", pid.trim())).exists() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("cancelled helper not reaped");
    }
}
#[cfg(not(feature = "inproc-encoder"))]
#[tokio::test]
async fn t116_idle_stream_provides_regular_decodable_join_points() {
    use tokio::io::AsyncWriteExt;
    for fps in [60, 90] {
        let mut manager = test_manager();
        manager.config.width = 32;
        manager.config.height = 32;
        manager.config.fps = fps;
        manager
            .encoder
            .start_with(&manager.config, manager.active_mode(), |command| {
                let mut args: Vec<_> = command.as_std().get_args().map(|a| a.to_owned()).collect();
                let input = args.iter().position(|a| a == "-i").unwrap() + 1;
                args[input] = "pipe:0".into();
                Command::new("ffmpeg")
                    .args(args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
            })
            .unwrap();
        let child = manager.encoder.child.as_mut().unwrap();
        let mut input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let started = Instant::now();
        let writer = async move {
            let frame = vec![128; 32 * 32 * 3 / 2];
            for _ in 0..18 {
                input.write_all(&frame).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        };
        let reader = collect_idle_join_points(output, started);
        let (_, keyframes) = tokio::time::timeout(std::time::Duration::from_secs(6), async {
            tokio::join!(writer, reader)
        })
        .await
        .unwrap();
        assert!(child.wait().await.unwrap().success());
        assert!(
            keyframes.len() >= 3,
            "{fps} fps target produced only {} idle join points",
            keyframes.len()
        );
        for pair in keyframes.windows(2) {
            assert!(pair[1].0 - pair[0].0 < std::time::Duration::from_millis(1600));
        }
        assert!(
            started.elapsed() - keyframes.last().unwrap().0
                < std::time::Duration::from_millis(1600)
        );
        // Every join point must carry current SPS/PPS and decode independently.
        for (_, data) in keyframes {
            let mut decoder = Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "h264",
                    "-i",
                    "pipe:0",
                    "-frames:v",
                    "1",
                    "-f",
                    "rawvideo",
                    "pipe:1",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            decoder
                .stdin
                .take()
                .unwrap()
                .write_all(&data)
                .await
                .unwrap();
            let decoded = decoder.wait_with_output().await.unwrap();
            assert!(
                decoded.status.success(),
                "{}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            assert_eq!(decoded.stdout.len(), 32 * 32 * 3 / 2);
        }
    }
}
#[cfg(not(feature = "inproc-encoder"))]
async fn collect_idle_join_points(
    mut output: tokio::process::ChildStdout,
    started: Instant,
) -> Vec<(std::time::Duration, Bytes)> {
    use tokio::io::AsyncReadExt;
    let mut parser = crate::annex_b::AnnexBPacketizer::new(Codec::H264, Default::default());
    let mut keyframes = Vec::new();
    let mut buffer = [0; 16384];
    loop {
        let n = output.read(&mut buffer).await.unwrap();
        if n == 0 {
            break;
        }
        for packet in parser.push(&buffer[..n]) {
            if packet.is_idr {
                keyframes.push((started.elapsed(), packet.data));
            }
        }
    }
    keyframes
}
fn test_manager() -> CaptureManager {
    static INSTANCE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100_000);
    let mut manager = CaptureManager::new(CaptureConfig {
        encoder: "libx264".into(),
        helper_path: "/nonexistent/uscreen-test-helper".into(),
        instance: INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ..CaptureConfig::default()
    });
    // Never address a real connector, even on a machine running UScreen.
    manager.helper.card = Some(u32::MAX);
    manager
}
async fn fake_helper() -> (Child, BufReader<tokio::process::ChildStdout>) {
    let mut child = Command::new("sh")
        .args([
            "-c",
            "trap 'echo stopped; exit 0' TERM; echo ready; while :; do sleep 0.02; done",
        ])
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    output.read_line(&mut ready).await.unwrap();
    assert_eq!(ready, "ready\n");
    (child, output)
}
fn fake_encoder(duration: &str) -> Child {
    Command::new("sleep")
        .arg(duration)
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}
async fn drive_session(manager: &mut CaptureManager, display: bool, change_mode: bool) {
    let c = &manager.config;
    let settings = EncoderSettings {
        encoder: c.encoder.clone(),
        fps: c.fps + u32::from(change_mode),
        bitrate: c.bitrate,
        width: c.width,
        height: c.height,
        quality: c.quality,
        width_mm: c.width_mm,
        height_mm: c.height_mm,
        stream_scale: c.stream_scale,
        geometry_ready: true,
    };
    let (_settings_tx, settings_rx) = watch::channel(settings);
    let (_display_tx, display_rx) = watch::channel(display);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let (tx, _rx) = broadcast::channel(8);
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        manager.stream_frames(tx, settings_rx, display_rx, shutdown_rx),
    )
    .await;
}
#[tokio::test]
async fn t018_waits_past_stale_none_until_stream_dimensions_arrive() {
    let manager = test_manager();
    manager.helper.stream_tx.send(None).unwrap();
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            manager.helper.wait_stream_size(&manager.config)
        )
        .await
        .is_err(),
        "a stale None is not a negotiated mode"
    );
    let send = async {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        manager.helper.stream_tx.send(Some((1280, 720))).unwrap();
    };
    let (result, _) = tokio::join!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            manager.helper.wait_stream_size(&manager.config)
        ),
        send
    );
    assert!(result.is_ok());
    assert_eq!(manager.active_mode(), (1280, 720));
}
#[tokio::test]
async fn t019_encoder_setup_failure_preserves_live_helper() {
    let mut manager = test_manager();
    let (child, _output) = fake_helper().await;
    let pid = child.id();
    manager.helper.child = Some(child);
    manager.config.encoder = "invalid-encoder".into();
    assert!(manager.start_session_encoder().await.is_err());
    assert_eq!(manager.helper.child.as_ref().and_then(Child::id), pid);
    assert!(manager
        .helper
        .child
        .as_mut()
        .unwrap()
        .try_wait()
        .unwrap()
        .is_none());
    manager.shutdown().await;
}
#[tokio::test]
async fn t020_encoder_records_the_dimensions_passed_to_its_process() {
    let mut manager = test_manager();
    manager.helper.stream_tx.send(Some((1280, 720))).unwrap();
    let stream_tx = manager.helper.stream_tx.clone();
    let used = manager
        .encoder
        .start_with(&manager.config, manager.active_mode(), |command| {
            let args: Vec<_> = command.as_std().get_args().collect();
            assert!(args.windows(2).any(|pair| pair == ["-s", "1280x720"]));
            // A helper announcement races with spawning the encoder.
            stream_tx.send(Some((1920, 1080))).unwrap();
            Ok(fake_encoder("5"))
        })
        .unwrap();
    manager.shutdown().await;
    assert_eq!(used, (1280, 720));
}
#[tokio::test]
async fn t054_vaapi_uses_the_configured_render_node() {
    let mut manager = test_manager();
    manager.config.encoder = "h264_vaapi".into();
    manager.config.vaapi_device = "/dev/dri/renderD129".into();
    let mut args = Vec::new();
    manager
        .encoder
        .start_with(&manager.config, manager.active_mode(), |command| {
            args = command
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            Ok(fake_encoder("5"))
        })
        .unwrap();
    manager.shutdown().await;
    assert!(args
        .windows(2)
        .any(|p| p == ["-vaapi_device", "/dev/dri/renderD129"]));
}
#[tokio::test]
async fn t055_hevc_vaapi_supports_eight_and_ten_bit_output() {
    for ten_bit in [false, true] {
        let mut manager = test_manager();
        manager.config.encoder = "hevc_vaapi".into();
        manager.config.ten_bit = ten_bit;
        let mut args = Vec::new();
        let result =
            manager
                .encoder
                .start_with(&manager.config, manager.active_mode(), |command| {
                    args = command
                        .as_std()
                        .get_args()
                        .map(|a| a.to_string_lossy().into_owned())
                        .collect();
                    Ok(fake_encoder("5"))
                });
        manager.shutdown().await;
        result.unwrap();
        assert!(args.windows(2).any(|p| p == ["-c:v", "hevc_vaapi"]));
        assert!(args.windows(2).any(|p| p == ["-f", "hevc"]));
        let filters: Vec<_> = args
            .windows(2)
            .filter(|p| p[0] == "-vf")
            .map(|p| p[1].as_str())
            .collect();
        assert_eq!(
            filters,
            [if ten_bit {
                "format=p010le,hwupload"
            } else {
                "format=nv12,hwupload"
            }]
        );
    }
}
#[tokio::test]
async fn t021_helper_exit_restarts_session_while_encoder_is_still_alive() {
    let mut manager = test_manager();
    manager.helper.stream_tx.send(Some((1280, 720))).unwrap();
    manager.helper.child = Some(fake_encoder("0.02"));
    manager.encoder.child = Some(fake_encoder("5"));
    drive_session(&mut manager, true, false).await;
    let restarted = manager.encoder.child.is_none();
    manager.shutdown().await;
    assert!(
        restarted,
        "a dead helper must end the session before the encoder exits"
    );
}
#[tokio::test]
async fn t022_helper_disconnects_cleanly_on_display_off_mode_change_and_crash() {
    for (display, change_mode) in [(false, false), (false, true), (true, false)] {
        let mut manager = test_manager();
        let (child, mut output) = fake_helper().await;
        manager.helper.child = Some(child);
        manager.helper.stream_tx.send(Some((1280, 720))).unwrap();
        if display {
            manager.encoder.child = Some(fake_encoder("0.02"));
        }
        drive_session(&mut manager, display, change_mode).await;
        manager.shutdown().await;
        let mut messages = String::new();
        output.read_to_string(&mut messages).await.unwrap();
        assert_eq!(
            messages, "stopped\n",
            "display={display}, change_mode={change_mode}"
        );
    }
}
