//! T226: partial production C writes followed by real CLI/libavcodec encoding.
use super::*;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

const NAME: &str = "capture::fifo_tests::t226_partial_frame_recovers_without_display_hotplug";

fn isolated_fixture() -> bool {
    if std::env::var_os("USCREEN_T226_ROOT").is_some() {
        return false;
    }
    let root = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    card_allocation_tests::compile_helper(root.path());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME, "--nocapture"])
        .env("USCREEN_T226_ROOT", root.path())
        .env("HOME", root.path())
        .env("XDG_RUNTIME_DIR", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "T226: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

async fn wait_for_marker(root: &Path, marker: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        while !root.join(marker).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("T226: helper did not exercise a stalled partial write");
}

fn settings() -> EncoderSettings {
    EncoderSettings {
        encoder: "libx264".into(),
        fps: 20,
        bitrate: 20000,
        width: 1024,
        height: 1024,
        quality: 20,
        width_mm: 200,
        height_mm: 200,
        stream_scale: 1,
        geometry_ready: true,
        decoders: None,
    }
}

async fn decode(packet: VideoPacket) -> Vec<u8> {
    let mut decoder = tokio::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "h264",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-pix_fmt",
            "nv12",
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
        .write_all(&packet.data)
        .await
        .unwrap();
    let result = decoder.wait_with_output().await.unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

async fn live_recovery(
    root: &Path,
    first: &VideoPacket,
    rx: &mut broadcast::Receiver<VideoPacket>,
) -> VideoPacket {
    std::fs::write(root.join("request-live"), "").unwrap();
    let next = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let packet = rx.recv().await.unwrap();
            if !Arc::ptr_eq(&packet.generation, &first.generation) {
                break packet;
            }
        }
    })
    .await
    .expect("T226: active encoder never recovered");
    assert!(
        !first.generation.load(std::sync::atomic::Ordering::Acquire),
        "T226: old frames still active"
    );
    assert!(next.seq > first.seq);
    std::fs::write(root.join("request-stale"), "").unwrap();
    wait_for_marker(root, "stale").await;
    for _ in 0..4 {
        let packet = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            Arc::ptr_eq(&packet.generation, &next.generation),
            "T226: stale reset retired the fresh encoder"
        );
    }
    next
}

fn assert_fresh_frame(decoded: &[u8]) {
    assert_eq!(decoded.len(), 1024 * 1024 * 3 / 2);
    let luma = &decoded[..1024 * 1024];
    let wrong = luma.iter().filter(|&&v| v.abs_diff(160) > 2).count();
    assert_eq!(
        wrong, 0,
        "T226: encoded a mixed frame from the retired FIFO"
    );
}

#[tokio::test]
async fn t226_partial_frame_recovers_without_display_hotplug() {
    if isolated_fixture() {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("USCREEN_T226_ROOT").unwrap());
    let mut manager = CaptureManager::new(CaptureConfig {
        helper_path: root.join("evdi_helper"),
        edid_path: Some(root.join("unused.edid")),
        encoder: "libx264".into(),
        width: 1024,
        height: 1024,
        fps: 20,
        width_mm: 200,
        height_mm: 200,
        quality: 20,
        ..Default::default()
    });
    manager.start_helper().await.unwrap();
    let old_reader = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(fifo_path_for(0).unwrap())
        .unwrap();
    wait_for_marker(&root, "partial").await;
    let (tx, mut rx) = crate::video_queue::channel(16, Default::default());
    let (_settings, settings_rx) = watch::channel(settings());
    let (_display, display_rx) = watch::channel(true);
    let (stop, stop_rx) = watch::channel(false);
    let session = tokio::spawn(async move {
        manager
            .stream_frames(tx, settings_rx, display_rx, stop_rx)
            .await
    });
    let packet = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await;
    let packet = packet.expect("T226: no recovery frame").unwrap();
    let second = live_recovery(&root, &packet, &mut rx).await;
    stop.send(true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(4), session)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    drop(old_reader);
    assert_fresh_frame(&decode(packet).await);
    assert_fresh_frame(&decode(second).await);
    assert_eq!(
        std::fs::read_to_string(root.join("starts")).unwrap(),
        "ready\n",
        "T226: recovery detached and reattached the virtual display"
    );
}
