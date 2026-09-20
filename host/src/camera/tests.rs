use super::*;
use clap::Parser;
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) fn options() -> CameraOptions {
    let cli = uscreen_config::cli::Cli::parse_from([
        "uscreen", "cameras", "--width", "160", "--height", "120",
    ]);
    let Some(uscreen_config::cli::Commands::Cameras(options)) = cli.command else {
        panic!()
    };
    options
}

pub(super) fn script(root: &Path, name: &str, source: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, format!("#!/usr/bin/python3\n{source}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[tokio::test]
async fn t539_graceful_output_stop_allows_final_blank_to_flush() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("flushed");
    let tool = script(root.path(), "writer", &format!("import sys, pathlib, time\nsys.stdin.buffer.read()\ntime.sleep(0.02)\npathlib.Path({:?}).write_text('flushed')", marker));
    let mut child = tokio::process::Command::new(tool)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take());
    outputs::retire(&mut child).await;
    assert!(
        marker.exists(),
        "T539 output killed before final frame was flushed"
    );
}

#[tokio::test]
async fn t539_real_h264_upload_decodes_only_to_selected_endpoint() {
    let profile = options();
    let ffmpeg = executable("ffmpeg").unwrap();
    let encoded = tokio::process::Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=160x120:r=30",
            "-frames:v",
            "8",
            "-c:v",
            "libx264",
            "-threads",
            "1",
            "-tune",
            "zerolatency",
            "-f",
            "h264",
            "pipe:1",
        ])
        .output()
        .await
        .unwrap();
    assert!(encoded.status.success());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (server, _) = listener.accept().await.unwrap();
    let (front, mut front_rx) = watch::channel(None);
    let (rear, rear_rx) = watch::channel(None);
    let outputs: [outputs::Frames; 2] = [front, rear];
    let token = "a".repeat(64);
    let upload = async {
        client
            .write_all(&[protocol::MAGIC.as_slice(), token.as_bytes(), &[0, 0]].concat())
            .await
            .unwrap();
        let mut ack = [0; 2];
        client.read_exact(&mut ack).await.unwrap();
        assert_eq!(&ack, b"OK");
        client.write_u32(encoded.stdout.len() as u32).await.unwrap();
        client.write_all(&encoded.stdout).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), front_rx.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            front_rx.borrow().as_ref().unwrap().len(),
            profile.frame_bytes()
        );
        assert!(rear_rx.borrow().is_none());
        client.shutdown().await.unwrap();
    };
    let (result, _) = tokio::join!(
        decoder::connection(server, &token, &ffmpeg, &profile, &outputs),
        upload
    );
    assert!(result.is_err());
    assert!(outputs[0].borrow().is_none());
}

#[tokio::test]
async fn t539_virtual_camera_writes_black_on_start_and_stop() {
    let root = tempfile::tempdir().unwrap();
    let capture = root.path().join("frames");
    let tool = script(
        root.path(),
        "writer",
        &format!(
            "import sys, pathlib\npathlib.Path({:?}).write_bytes(sys.stdin.buffer.read())",
            capture
        ),
    );
    let profile = options();
    let mut child = outputs::producer(&tool, Path::new("/unused"), &profile)
        .spawn()
        .unwrap();
    let (frames, receiver) = watch::channel(None);
    let stop = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(frames);
    };
    let (result, _) = tokio::join!(outputs::feed(&mut child, receiver, &profile), stop);
    assert!(result.is_err());
    outputs::retire(&mut child).await;
    let bytes = std::fs::read(capture).unwrap();
    assert!(bytes.len() >= profile.frame_bytes() * 2);
    assert_eq!(
        &bytes[bytes.len() - profile.frame_bytes()..],
        &outputs::blank(&profile)
    );
}

#[tokio::test]
async fn t539_bridge_invitation_and_mapping_ownership() {
    let root = tempfile::tempdir().unwrap();
    let adb = script(root.path(), "adb", "import sys\na=sys.argv\nif 'get-serialno' in a: print('tablet')\nelif '--list' in a: print('Usb tcp:34567 tcp:12345')\nelif 'tcp:0' in a: print('34567')\nelif 'broadcast' in a: print('Broadcast completed: result=1')");
    let bridge = bridge::Bridge::create(adb.clone(), None, 12345)
        .await
        .unwrap();
    assert_eq!(bridge.serial, "tablet");
    bridge.invite(&"b".repeat(64), &options()).await.unwrap();
    bridge.close().await.unwrap();
    let explicit = bridge::Bridge::create(adb, Some("selected"), 12345)
        .await
        .unwrap();
    assert_eq!(explicit.serial, "selected");
    explicit.close().await.unwrap();
    let bad = script(
        root.path(),
        "bad",
        "import sys\nprint('failed',file=sys.stderr)\nsys.exit(1)",
    );
    assert!(bridge::Bridge::create(bad, None, 12345).await.is_err());
}

#[test]
fn t539_rotation_and_mirroring_are_bounded_filter_choices() {
    let mut profile = options();
    for (rotation, prefix) in [
        (0, "scale="),
        (1, "transpose=clock,"),
        (2, "hflip,vflip,"),
        (3, "transpose=cclock,"),
    ] {
        assert!(decoder::filters(rotation, &profile).starts_with(prefix));
    }
    profile.mirror = true;
    assert!(decoder::filters(0, &profile).starts_with("hflip,scale="));
    assert!(executable("uscreen-t539-missing-executable").is_err());
}

#[tokio::test]
async fn t539_rejects_encoded_dimensions_above_negotiated_profile() {
    let profile = options();
    let ffmpeg = executable("ffmpeg").unwrap();
    let encoded = tokio::process::Command::new(&ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=320x240:r=30",
            "-frames:v",
            "8",
            "-c:v",
            "libx264",
            "-threads",
            "1",
            "-tune",
            "zerolatency",
            "-f",
            "h264",
            "pipe:1",
        ])
        .output()
        .await
        .unwrap();
    assert!(encoded.status.success());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (server, _) = listener.accept().await.unwrap();
    let (front, mut received) = watch::channel(None);
    let (rear, _) = watch::channel(None);
    let outputs: [outputs::Frames; 2] = [front, rear];
    let token = "a".repeat(64);
    let upload = async {
        client
            .write_all(&[protocol::MAGIC.as_slice(), token.as_bytes(), &[0, 0]].concat())
            .await
            .unwrap();
        client.read_exact(&mut [0; 2]).await.unwrap();
        client.write_u32(encoded.stdout.len() as u32).await.unwrap();
        client.write_all(&encoded.stdout).await.unwrap();
        tokio::time::timeout(Duration::from_secs(6), received.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(
            received.borrow().is_none(),
            "T539 oversized H.264 frame accepted despite negotiated profile"
        );
        let _ = client.shutdown().await;
    };
    let (result, _) = tokio::join!(
        decoder::connection(server, &token, &ffmpeg, &profile, &outputs),
        upload
    );
    assert!(result.is_err());
}

#[tokio::test]
async fn t539_stalled_decoder_cannot_hold_camera_session_forever() {
    let root = tempfile::tempdir().unwrap();
    let decoder = script(root.path(), "stalled", "import time\ntime.sleep(30)");
    let profile = options();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (server, _) = listener.accept().await.unwrap();
    let (front, _) = watch::channel(None);
    let (rear, _) = watch::channel(None);
    let frames: [outputs::Frames; 2] = [front, rear];
    let token = "a".repeat(64);
    let upload = async {
        client
            .write_all(&[protocol::MAGIC.as_slice(), token.as_bytes(), &[0, 0]].concat())
            .await
            .unwrap();
        client.read_exact(&mut [0; 2]).await.unwrap();
        client.write_u32(protocol::MAX_PACKET as u32).await.unwrap();
        client
            .write_all(&vec![0; protocol::MAX_PACKET])
            .await
            .unwrap();
        client.shutdown().await.unwrap();
    };
    let completed = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            decoder::connection(server, &token, &decoder, &profile, &frames),
            upload
        )
    })
    .await;
    assert!(
        completed.is_ok(),
        "T539 decoder blocked past camera session deadline"
    );
    assert!(completed.unwrap().0.is_err());
}

#[tokio::test]
async fn t539_producer_failure_retires_session_and_rejects_foreign_clients() {
    let root = tempfile::tempdir().unwrap();
    let adb = script(root.path(), "adb", "print('Broadcast completed: result=1')");
    let ffmpeg = script(
        root.path(),
        "producer",
        "import signal, sys\nsignal.alarm(1)\nwhile sys.stdin.buffer.read(4096): pass",
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bridge = bridge::Bridge {
        adb,
        serial: "tablet".into(),
        remote: "tcp:34567".into(),
        local: "tcp:12345".into(),
    };
    let token = "a".repeat(64);
    let mut profile = options();
    profile.lens = uscreen_config::camera::Lens::Rear;
    let clients = async {
        let mut foreign = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        foreign.write_all(&[0; 74]).await.unwrap();
        assert_eq!(foreign.read(&mut [0]).await.unwrap(), 0);
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        client
            .write_all(&[protocol::MAGIC.as_slice(), token.as_bytes(), &[1, 0]].concat())
            .await
            .unwrap();
        let mut ack = [0; 2];
        client.read_exact(&mut ack).await.unwrap();
        assert_eq!(&ack, b"OK");
        client.shutdown().await.unwrap();
    };
    let (_stop, mut stopped) = watch::channel(false);
    let (status, _) = watch::channel(State::Starting);
    let status = Report {
        state: status,
        preview: watch::channel(None).0,
    };
    let (result, _) = tokio::join!(
        serve(
            &listener,
            &bridge,
            &token,
            &ffmpeg,
            &profile,
            &mut stopped,
            &status
        ),
        clients
    );
    assert!(result.is_err());
    let mut invalid = profile;
    invalid.front_device = root.path().join("missing");
    assert!(run(&invalid).await.is_err());
}

#[tokio::test]
async fn t543_host_reports_frames_and_rejects_unselected_lens() {
    let profile = options();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (server, _) = listener.accept().await.unwrap();
    let (front, _) = watch::channel(None);
    let (rear, _) = watch::channel(None);
    let frames = [front, rear];
    let token = "a".repeat(64);
    client
        .write_all(&[protocol::MAGIC.as_slice(), token.as_bytes(), &[1, 0]].concat())
        .await
        .unwrap();
    let result = decoder::connection(
        server,
        &token,
        Path::new("/missing-ffmpeg"),
        &profile,
        &frames,
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("selected by host"));
    let (status, mut state) = watch::channel(State::Waiting);
    let status = Report {
        state: status,
        preview: watch::channel(None).0,
    };
    let monitor = frame_status(&frames, &profile, &status);
    tokio::pin!(monitor);
    let checks = async {
        tokio::task::yield_now().await;
        frames[0].send_replace(Some(std::sync::Arc::new(outputs::blank(&profile))));
        state.wait_for(|s| *s == State::Streaming).await.unwrap();
        assert!(status.preview.borrow().is_some());
        frames[0].send_replace(None);
        state.wait_for(|s| *s == State::Waiting).await.unwrap();
        assert!(status.preview.borrow().is_none());
    };
    tokio::select! { _ = &mut monitor => panic!(), _ = checks => {} }
}

#[tokio::test]
async fn t543_embedded_adapter_rejects_invalid_profile_before_native_work() {
    let (_stop, stopped) = watch::channel(false);
    let (status, _) = watch::channel(State::Stopped);
    let status = Report {
        state: status,
        preview: watch::channel(None).0,
    };
    let profile = CameraProfile {
        fps: 0,
        ..Default::default()
    };
    assert!(run_controlled(profile, stopped, status)
        .await
        .unwrap_err()
        .to_string()
        .contains("FPS"));
}

#[test]
fn t543_desktop_rotation_combines_sensor_orientation_before_mirroring() {
    let mut profile = options();
    for host in [0, 90, 180, 270] {
        profile.rotation = host;
        for sensor in 0..4 {
            let expected = match (sensor + (host / 90) as u8) % 4 {
                0 => "scale=",
                1 => "transpose=clock,",
                2 => "hflip,vflip,",
                _ => "transpose=cclock,",
            };
            assert!(decoder::filters(sensor, &profile).starts_with(expected));
        }
    }
    profile.rotation = 180;
    profile.mirror = true;
    assert!(decoder::filters(0, &profile).starts_with("hflip,vflip,hflip,"));
}

async fn transformed_test_frame(
    encoded: &[u8],
    profile: &CameraOptions,
) -> std::sync::Arc<Vec<u8>> {
    let ffmpeg = executable("ffmpeg").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (server, _) = listener.accept().await.unwrap();
    let (front, mut received) = watch::channel(None);
    let frames = [front, watch::channel(None).0];
    let token = "a".repeat(64);
    let upload = async {
        client
            .write_all(&[protocol::MAGIC.as_slice(), token.as_bytes(), &[0, 0]].concat())
            .await
            .unwrap();
        client.read_exact(&mut [0; 2]).await.unwrap();
        client.write_u32(encoded.len() as u32).await.unwrap();
        client.write_all(encoded).await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(3),
            received.wait_for(|image| image.is_some()),
        )
        .await
        .unwrap()
        .unwrap()
        .clone()
        .unwrap()
    };
    tokio::select! {
        result = decoder::connection(server, &token, &ffmpeg, profile, &frames) => panic!("{result:?}"),
        frame = upload => frame,
    }
}

#[tokio::test]
async fn t543_real_output_and_preview_rotate_the_same_asymmetric_picture() {
    let encoded = tokio::process::Command::new(executable("ffmpeg").unwrap())
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=red:s=160x120:r=30,drawbox=x=0:y=60:w=160:h=60:color=blue:t=fill",
            "-frames:v",
            "8",
            "-c:v",
            "libx264",
            "-threads",
            "1",
            "-tune",
            "zerolatency",
            "-f",
            "h264",
            "pipe:1",
        ])
        .output()
        .await
        .unwrap();
    assert!(encoded.status.success());
    // A clockwise quarter-turn moves the red top half to the right.
    for (rotation, red, blue) in [
        (0, (80, 30), (80, 90)),
        (90, (115, 60), (45, 60)),
        (180, (80, 90), (80, 30)),
        (270, (45, 60), (115, 60)),
    ] {
        let mut profile = options();
        profile.rotation = rotation;
        let bytes = transformed_test_frame(&encoded.stdout, &profile).await;
        let preview = uscreen_config::camera::CameraPreview::from_yuv420(
            profile.lens,
            profile.width,
            profile.height,
            &bytes,
        )
        .unwrap();
        let channel = |point: (usize, usize), c: usize| {
            i16::from(preview.rgba()[(point.1 * 160 + point.0) * 4 + c])
        };
        assert!(
            channel(red, 0) - channel(red, 2) > 100,
            "T543 red quadrant wrong at {rotation}°"
        );
        assert!(
            channel(blue, 2) - channel(blue, 0) > 100,
            "T543 blue quadrant wrong at {rotation}°"
        );
    }
}
