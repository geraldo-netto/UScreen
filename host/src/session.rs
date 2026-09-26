//! Linux construction of the shared session and native capture adapter.
use crate::{capture, input, media};
#[cfg(test)]
pub(crate) use blent::session::start_servers;
pub(crate) use blent::session::Runtime;
#[cfg(test)]
use blent::session::{forward_shutdown, spawn_display_gate, spawn_server};
use blent::session::{CaptureBackend, CaptureContext, CaptureResources, CaptureWorkers, Prepared};
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;
use tokio::sync::watch;
#[cfg(test)]
use tokio::task::JoinHandle;
use tracing::error;

pub(crate) struct Spec {
    pub capture: capture::CaptureConfig,
    pub ports: (u16, u16),
    pub token: Option<String>,
    pub devices: (bool, bool, bool),
}

impl Spec {
    fn settings(&self) -> media::EncoderSettings {
        let cfg = &self.capture;
        media::EncoderSettings {
            encoder: cfg.encoder.clone(),
            fps: cfg.fps,
            bitrate: cfg.bitrate,
            width: cfg.width,
            height: cfg.height,
            quality: cfg.quality,
            width_mm: cfg.width_mm,
            height_mm: cfg.height_mm,
            stream_scale: cfg.stream_scale,
            geometry_ready: false,
            decoders: None,
            decoder_epoch: 0,
            selection: None,
        }
    }

    pub fn prepare(self, mode: watch::Sender<bool>) -> Prepared {
        let settings = self.settings();
        let manager = capture::CaptureManager::new(self.capture.clone());
        let backend = Arc::new(input::LinuxBackend::new(manager.card_rx()));
        blent::session::Spec {
            settings,
            instance: self.capture.instance,
            ports: self.ports,
            token: self.token,
            devices: self.devices,
        }
        .prepare(
            mode,
            Box::new(LinuxCapture {
                manager,
                config: self.capture,
            }),
            backend,
        )
    }
}

struct LinuxCapture {
    manager: capture::CaptureManager,
    config: capture::CaptureConfig,
}
impl CaptureBackend for LinuxCapture {
    fn resources(&self) -> CaptureResources {
        CaptureResources {
            codec_config: self.manager.codec_config(),
            idr_wanted: self.manager.idr_request_flag(),
            latency: self.manager.latency_tracker(),
        }
    }
    fn start(self: Box<Self>, context: CaptureContext) -> CaptureWorkers {
        let Self {
            mut manager,
            config,
        } = *self;
        #[cfg(not(feature = "inproc-encoder"))]
        let selector = crate::selection::spawn(
            config.clone(),
            context.settings,
            context.display.clone(),
            context.stop.clone(),
            manager.latency_tracker(),
            context.attachment,
        );
        let capture = tokio::spawn(async move {
            if let Err(error) = manager
                .stream_frames(
                    context.video,
                    context.settings_rx,
                    context.display,
                    context.stop,
                )
                .await
            {
                error!("Capture manager {} failed: {error}", config.instance);
            }
        });
        CaptureWorkers {
            capture,
            #[cfg(not(feature = "inproc-encoder"))]
            auxiliary: vec![selector],
            #[cfg(feature = "inproc-encoder")]
            auxiliary: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn t450_initial_or_already_seen_stop_is_forwarded() {
        for daemon_stops in [true, false] {
            for initially_set in [true, false] {
                let (daemon, mut daemon_rx) = watch::channel(daemon_stops && initially_set);
                let (local, mut local_rx) = watch::channel(!daemon_stops && initially_set);
                if !initially_set {
                    if daemon_stops {
                        daemon.send(true).unwrap();
                    } else {
                        local.send(true).unwrap();
                    }
                    daemon_rx.borrow_and_update();
                    local_rx.borrow_and_update();
                }
                let (capture, mut capture_rx) = watch::channel(false);
                let task = forward_shutdown(daemon_rx, local_rx, capture);
                let stopped = tokio::time::timeout(
                    Duration::from_millis(100),
                    capture_rx.wait_for(|stop| *stop),
                )
                .await;
                task.abort();
                assert!(
                    matches!(stopped, Ok(Ok(_))),
                    "T450: already-requested stop was missed"
                );
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn t450_false_update_keeps_capture_alive_until_true_or_channel_close() {
        for close in [false, true] {
            let (daemon, daemon_rx) = watch::channel(false);
            let (_local, local_rx) = watch::channel(false);
            let (capture, mut capture_rx) = watch::channel(false);
            let task = forward_shutdown(daemon_rx, local_rx, capture);
            daemon.send(false).unwrap();
            tokio::task::yield_now().await;
            assert!(
                !*capture_rx.borrow(),
                "T450: false is not a shutdown request"
            );
            if !close {
                daemon.send(true).unwrap();
            }
            drop(daemon);
            let stopped = tokio::time::timeout(
                Duration::from_millis(100),
                capture_rx.wait_for(|stop| *stop),
            )
            .await;
            task.abort();
            assert!(
                matches!(stopped, Ok(Ok(_))),
                "T450: stop/closure was missed"
            );
        }
    }

    fn spec(ports: (u16, u16)) -> Spec {
        Spec {
            capture: capture::CaptureConfig {
                helper_path: "/nonexistent-t368-helper".into(),
                ..Default::default()
            },
            ports,
            token: None,
            devices: (false, false, false),
        }
    }

    #[tokio::test]
    async fn t281_coalesced_detach_attach_cannot_reuse_previous_geometry() {
        let (mode, mode_rx) = watch::channel(false);
        let prepared = spec((0, 0)).prepare(mode);
        let tablet = prepared.tablet;
        let settings = prepared.settings;
        let (gate, active) = watch::channel(false);
        let task = spawn_display_gate(gate, tablet.subscribe(), mode_rx);
        let _ = tablet.send(true);
        settings.send_modify(|s| s.geometry_ready = true);
        tokio::task::yield_now().await;
        assert!(*active.borrow());
        // Both notifications arrive before the consumer gets scheduled.
        let _ = tablet.send(false);
        let _ = tablet.send(true);
        tokio::task::yield_now().await;
        assert!(
            !settings.borrow().geometry_ready,
            "T281: a replacement must negotiate fresh geometry even when presence coalesces"
        );
        task.abort();
    }

    #[tokio::test]
    async fn t281_current_metadata_survives_delayed_gate_and_transport_migration() {
        let (mode, mode_rx) = watch::channel(false);
        let prepared = spec((0, 0)).prepare(mode);
        let tablet = prepared.tablet;
        let settings = prepared.settings;
        let (gate, active) = watch::channel(false);
        let task = spawn_display_gate(gate, tablet.subscribe(), mode_rx);
        tablet.begin(Some("tablet-a".into()));
        let previous = tablet.lease();
        assert!(previous.apply(|| settings.send_modify(|s| s.geometry_ready = true)));
        tablet.send(true).unwrap();
        tokio::task::yield_now().await;
        // Replace without yielding; old authenticated metadata is rejected.
        tablet.begin(Some("tablet-b".into()));
        assert!(!settings.borrow().geometry_ready);
        assert!(!previous.apply(|| settings.send_modify(|s| s.geometry_ready = true)));
        let current = tablet.lease();
        current.apply(|| {
            settings.send_modify(|s| {
                s.width = 1280;
                s.height = 800;
                s.width_mm = 240;
                s.height_mm = 150;
                s.geometry_ready = true;
            })
        });
        tablet.send(true).unwrap();
        tokio::task::yield_now().await;
        assert!(*active.borrow());
        assert!(
            settings.borrow().geometry_ready,
            "T281: gate cleared current metadata"
        );
        assert_eq!(
            (settings.borrow().width, settings.borrow().height),
            (1280, 800)
        );
        assert_eq!(
            (settings.borrow().width_mm, settings.borrow().height_mm),
            (240, 150)
        );
        // Same physical tablet, new transport: preserve geometry but retire its
        // previous connection. Explicit absence requires a fresh negotiation.
        tablet.begin(Some("tablet-b".into()));
        assert!(settings.borrow().geometry_ready);
        assert!(!current.apply(|| panic!("T281: old transport lease survived")));
        tablet.send(false).unwrap();
        tablet.begin(Some("tablet-b".into()));
        assert!(!settings.borrow().geometry_ready);
        task.abort();
    }

    #[tokio::test]
    async fn t368_second_bind_failure_releases_first_listener() {
        let video = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let input = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ports = (
            video.local_addr().unwrap().port(),
            input.local_addr().unwrap().port(),
        );
        drop(video);
        let (mode, _) = watch::channel(false);
        let (_shutdown, shutdown_rx) = watch::channel(false);
        assert!(spec(ports).prepare(mode).start(shutdown_rx).await.is_err());
        tokio::net::TcpListener::bind(("127.0.0.1", ports.0))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn t368_runtime_owns_listeners_and_queued_initial_settings() {
        let video = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let input = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ports = (
            video.local_addr().unwrap().port(),
            input.local_addr().unwrap().port(),
        );
        drop((video, input));
        let (mode, _) = watch::channel(false);
        let (_shutdown, shutdown_rx) = watch::channel(false);
        let prepared = spec(ports).prepare(mode);
        let mut settings = prepared.settings.borrow().clone();
        settings.fps = 30;
        prepared.settings.send(settings).unwrap();
        assert!(prepared.settings_rx.has_changed().unwrap());
        let runtime = prepared.start(shutdown_rx).await.unwrap();
        assert!(tokio::net::TcpListener::bind(("127.0.0.1", ports.0))
            .await
            .is_err());
        runtime.stop().await;
        tokio::net::TcpListener::bind(("127.0.0.1", ports.0))
            .await
            .unwrap();
        tokio::net::TcpListener::bind(("127.0.0.1", ports.1))
            .await
            .unwrap();
    }
}

#[cfg(test)]
mod credential_tests;

#[cfg(test)]
#[tokio::test]
async fn t497_server_task_errors_keep_the_service_name_and_do_not_escape() {
    const TEST: &str = "session::t497_server_task_errors_keep_the_service_name_and_do_not_escape";
    if std::env::var_os("BLENT_T497_SERVER_REPORT").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env("BLENT_T497_SERVER_REPORT", "1")
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
    spawn_server("Successful", async { Ok(()) }).await.unwrap();
    assert!(!crate::test_logging::text().contains("Successful server failed"));
    for name in ["Stream", "Input"] {
        spawn_server(name, async { anyhow::bail!("fixture failure") })
            .await
            .unwrap();
        assert!(
            crate::test_logging::text().contains(&format!("{name} server failed: fixture failure"))
        );
    }
}
