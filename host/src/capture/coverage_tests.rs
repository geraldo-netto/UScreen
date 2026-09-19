//! T497: supervisor event contracts with owned idle processes, never real capture.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;
use tokio::process::Command;

const TEST: &str =
    "capture::coverage_tests::t497_supervisor_distinguishes_notifications_restarts_and_failures";

struct Fixture {
    manager: CaptureManager,
    run: CaptureRun,
    settings: watch::Sender<EncoderSettings>,
    display: watch::Sender<bool>,
    _shutdown: watch::Sender<bool>,
}

fn idle_child() -> tokio::process::Child {
    Command::new("/bin/sleep")
        .arg("60")
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

impl Fixture {
    fn new() -> Self {
        let manager = CaptureManager::new(CaptureConfig {
            encoder: "libx264".into(),
            width: 64,
            height: 64,
            instance: u32::MAX,
            helper_path: "/never/execute/an/evdi/helper".into(),
            ..Default::default()
        });
        let settings = EncoderSettings {
            encoder: "libx264".into(),
            fps: 60,
            bitrate: 20_000,
            width: 64,
            height: 64,
            quality: crate::config::DEFAULT_QUALITY,
            width_mm: manager.config.width_mm,
            height_mm: manager.config.height_mm,
            stream_scale: 1,
            geometry_ready: true,
            decoders: None,
            decoder_epoch: 0,
            selection: None,
        };
        let (settings, settings_rx) = watch::channel(settings);
        let (display, display_rx) = watch::channel(true);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let run = CaptureRun {
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
        Self {
            manager,
            run,
            settings,
            display,
            _shutdown: shutdown,
        }
    }

    fn children(&mut self) {
        self.manager.helper.child = Some(idle_child());
        self.manager.helper.card = Some(u32::MAX);
        self.manager.encoder.child = Some(idle_child());
    }

    async fn change_mode(&mut self, stream: bool) {
        self.children();
        let (video, _receiver) = crate::video_queue::channel(8, Default::default());
        let stream_tx = self.manager.helper.stream_tx.clone();
        let mode_tx = self.manager.helper.mode_tx.clone();
        let mut task = Box::pin(self.manager.run_encoder_session(&video, &mut self.run));
        assert!(futures_util::poll!(&mut task).is_pending());
        for size in [64, 128] {
            if stream {
                stream_tx.send(Some((size, size))).unwrap();
            } else {
                mode_tx
                    .send(Some(DetectedMode {
                        width: size,
                        height: size,
                        refresh: 60,
                    }))
                    .unwrap();
            }
            if size == 64 {
                assert!(
                    futures_util::poll!(&mut task).is_pending(),
                    "unchanged dimensions restarted encoder"
                );
            }
        }
        let changes = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(changes.mode_changed);
        assert!(!changes.settings_changed);
        assert!(!changes.display_dropped);
        assert!(changes.keep_helper());
        self.manager.shutdown().await;
    }

    async fn display_notifications(&mut self) {
        self.children();
        let (video, _receiver) = crate::video_queue::channel(8, Default::default());
        let mut task = Box::pin(self.manager.run_encoder_session(&video, &mut self.run));
        assert!(futures_util::poll!(&mut task).is_pending());
        self.display.send(true).unwrap();
        assert!(
            futures_util::poll!(&mut task).is_pending(),
            "same active display restarted encoder"
        );
        self.settings
            .send_modify(|settings| settings.decoder_epoch += 1);
        assert!(
            futures_util::poll!(&mut task).is_pending(),
            "metadata-only change restarted encoder"
        );
        self.display.send(false).unwrap();
        let changes = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(changes.display_dropped);
        assert!(!changes.keep_helper());
        self.manager.shutdown().await;
    }

    async fn failed_start(&mut self) {
        self.manager.config.encoder = "invalid-encoder".into();
        assert!(!self.manager.ensure_session_encoder(&mut self.run).await);
        assert!(!self.manager.encoder.is_running());
        self.display.send(false).unwrap();
        assert!(!self.manager.ensure_session_encoder(&mut self.run).await);
        assert!(crate::test_logging::text().contains("Failed to start encoder"));
    }

    fn explain_once(&mut self) {
        self.run.explain_evdi_failure_with(|| None);
        assert!(!self.run.explained_evdi);
        self.run
            .explain_evdi_failure_with(|| Some("T497 missing EVDI fixture".into()));
        self.run
            .explain_evdi_failure_with(|| panic!("explained failure must not be re-queried"));
        assert!(self.run.explained_evdi);
        self.run.explain_evdi_failure();
        assert_eq!(
            crate::test_logging::text()
                .matches("T497 missing EVDI fixture")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn t497_supervisor_distinguishes_notifications_restarts_and_failures() {
    if std::env::var_os("USCREEN_T497_CAPTURE_STATE").is_none() {
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env("USCREEN_T497_CAPTURE_STATE", "1")
            .env("HOME", root.path())
            .env("XDG_RUNTIME_DIR", root.path())
            .env("PATH", root.path())
            .env("XDG_CURRENT_DESKTOP", "T497 fixture")
            .env("XDG_SESSION_TYPE", "x11")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    crate::test_logging::enable();
    for stream in [false, true] {
        Fixture::new().change_mode(stream).await;
    }
    Fixture::new().display_notifications().await;
    let mut fixture = Fixture::new();
    fixture.failed_start().await;
    fixture.explain_once();
    assert!(
        !fifo_path_for(u32::MAX).unwrap().exists(),
        "supervisor fixture started capture"
    );
}
