use super::*;
use std::os::unix::ffi::OsStrExt;

fn private_runtime_root() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

#[test]
fn t455_inprocess_runtime_fixtures_survive_shared_umask() {
    for name in [
        "capture::inproc_tests::t284_vaapi_is_rejected_before_fifo_or_helper_creation",
        "capture::inproc_tests::t322_cancelled_inprocess_session_releases_fifo",
    ] {
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", "umask 0002; exec \"$@\"", "uscreen-t455"])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T455: {name}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
async fn t284_vaapi_is_rejected_before_fifo_or_helper_creation() {
    if std::env::var_os("USCREEN_T284_CHILD").is_none() {
        let root = private_runtime_root();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "capture::inproc_tests::t284_vaapi_is_rejected_before_fifo_or_helper_creation",
                "--nocapture",
            ])
            .env("USCREEN_T284_CHILD", "1")
            .env("XDG_RUNTIME_DIR", root.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "T284: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    for encoder in [
        "h264_vaapi",
        "h264_vaapi_baseline",
        "hevc_vaapi",
        "vaapih264enc",
    ] {
        let mut manager = CaptureManager::new(CaptureConfig {
            encoder: encoder.into(),
            helper_path: "/nonexistent-t284-helper".into(),
            edid_path: Some("unused-t284-edid".into()),
            ..Default::default()
        });
        let error = manager.start_helper().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("VAAPI is unavailable in this in-process build"),
            "T284: {error:#}"
        );
        assert!(
            !fifo_path_for(0).unwrap().exists(),
            "T284: invalid encoder claimed a FIFO"
        );
        assert!(manager.helper.child.is_none());
    }
}

fn fifo_is_open(path: &std::path::Path) -> bool {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .flatten()
        .any(|entry| std::fs::read_link(entry.path()).is_ok_and(|target| target == path))
}

async fn wait_for_fifo(path: &std::path::Path, open: bool) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while fifo_is_open(path) != open {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

async fn cancel_waiting_encoder() -> (bool, bool) {
    let mut manager = CaptureManager::new(CaptureConfig {
        encoder: "libx264".into(),
        width: 64,
        height: 64,
        ..Default::default()
    });
    let fifo = fifo_path_for(manager.config.instance).unwrap();
    let path = std::path::Path::new(&fifo);
    let cpath = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    let (_settings, settings_rx) = watch::channel(EncoderSettings {
        encoder: "libx264".into(),
        fps: 60,
        bitrate: 1000,
        width: 64,
        height: 64,
        quality: 20,
        width_mm: 310,
        height_mm: 194,
        stream_scale: 1,
        geometry_ready: true,
        decoders: None,
        decoder_epoch: 0,
        selection: None,
    });
    let (_display, display_rx) = watch::channel(true);
    let (_shutdown, shutdown_rx) = watch::channel(false);
    let mut run = CaptureRun {
        settings_rx,
        display_rx,
        shutdown_rx,
        mode_rx: manager.helper.mode_rx.clone(),
        stream_rx: manager.helper.stream_rx.clone(),
        fifo_reset_rx: manager.helper.fifo_reset_rx.clone(),
        backoff_ms: 0,
        explained_evdi: false,
        pipeline_started_at: Instant::now(),
        encoder_mode: Some((64, 64)),
    };
    let (video, _receiver) = crate::video_queue::channel(8, Default::default());
    // Enter the production session directly: no helper, display attachment,
    // compositor commands or uinput devices are involved.
    let mut session = Box::pin(manager.run_encoder_session(&video, &mut run));
    assert!(futures_util::poll!(&mut session).is_pending());
    let opened = wait_for_fifo(path, true).await;
    drop(session); // The same cancellation as aborting the owning task.
    let closed = wait_for_fifo(path, false).await;
    (opened, closed)
}

#[test]
fn t322_cancelled_inprocess_session_releases_fifo() {
    if std::env::var_os("USCREEN_T322_CHILD").is_none() {
        use uscreen_config::commands::SyncCommandExt;
        let root = private_runtime_root();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "capture::inproc_tests::t322_cancelled_inprocess_session_releases_fifo",
                "--nocapture",
            ])
            .env("USCREEN_T322_CHILD", "1")
            .env("XDG_RUNTIME_DIR", root.path())
            .output_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert!(
            result.status.success(),
            "T322: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (opened, closed) = runtime.block_on(cancel_waiting_encoder());
    let shutdown_at = Instant::now();
    // The bounded child can report a failure even if the old implementation
    // leaves a blocking worker alive; the process exit retires that worker.
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    assert!(opened, "T322: encoder never opened its private FIFO");
    assert!(closed, "T322: cancelled encoder still owns its FIFO reader");
    assert!(
        shutdown_at.elapsed() < std::time::Duration::from_secs(1),
        "T322: runtime shutdown waited for an orphan encoder"
    );
}
