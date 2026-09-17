//! Shared construction and ownership for primary and additional tablet sessions.
//! Persistence and CLI policy belong to the daemon, not an individual pipeline.
use crate::{capture, input, media, stream};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;
use tracing::{error, warn};

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
        }
    }

    fn input_config(&self) -> input::InputConfig {
        input::InputConfig {
            port: self.ports.1,
            instance: self.capture.instance,
            token: self.token.clone(),
            codec: media::Codec::from_encoder(&self.capture.encoder)
                .muxer()
                .into(),
            virtual_width: self.capture.width,
            virtual_height: self.capture.height,
            touch: self.devices.0,
            pen: self.devices.1,
            pointer: self.devices.2,
        }
    }

    /// No workers or sockets yet. Callers may snapshot settings for persistence
    /// before any control message can change them (T292/T296).
    pub fn prepare(self, mode: watch::Sender<bool>) -> Prepared {
        let (settings, settings_rx) = watch::channel(self.settings());
        let tablet = crate::attachment::Attachment::new(settings.clone());
        let relaunch = Arc::new(Notify::new());
        let input_config = self.input_config();
        let capture = capture::CaptureManager::new(self.capture.clone());
        let stream = stream::StreamServer::new(
            stream::StreamConfig {
                video_port: self.ports.0,
                token: self.token,
            },
            capture.codec_config_arc(),
            capture.idr_request_flag(),
        );
        let input = input::InputServer::new(
            input_config,
            Some(settings.clone()),
            mode.clone(),
            capture.latency_tracker(),
            relaunch.clone(),
            capture.card_rx(),
            tablet.subscribe(),
        )
        .with_attachment(tablet.clone());
        Prepared {
            settings,
            settings_rx,
            tablet,
            relaunch,
            mode,
            capture,
            stream,
            input,
            instance: self.capture.instance,
            ports: self.ports,
        }
    }
}

pub(crate) struct Prepared {
    pub settings: watch::Sender<media::EncoderSettings>,
    settings_rx: watch::Receiver<media::EncoderSettings>,
    pub tablet: crate::attachment::Attachment,
    pub relaunch: Arc<Notify>,
    mode: watch::Sender<bool>,
    capture: capture::CaptureManager,
    stream: stream::StreamServer,
    input: input::InputServer,
    instance: u32,
    ports: (u16, u16),
}

impl Prepared {
    pub async fn start(self, daemon_stop: watch::Receiver<bool>) -> Result<Runtime> {
        // Keep queues shallow; slow clients recover at an IDR instead of
        // accumulating seconds of queued frames.
        let (video, _) = crate::video_queue::channel(
            crate::video_queue::QUEUE_PACKETS,
            self.capture.idr_request_flag(),
        );
        let (stream, input) = start_servers(self.stream, self.input, video.clone()).await?;
        let (gate_tx, gate_rx) = watch::channel(false);
        let (stop_tx, stop_rx) = watch::channel(false);
        let (capture_stop, capture_stop_rx) = watch::channel(false);
        let settings_rx = self.settings_rx;
        let gate = spawn_display_gate(gate_tx, self.tablet.subscribe(), self.mode.subscribe());
        let stop = forward_shutdown(daemon_stop, stop_rx, capture_stop);
        let mut manager = self.capture;
        let instance = self.instance;
        let capture = tokio::spawn(async move {
            if let Err(error) = manager
                .stream_frames(video, settings_rx, gate_rx, capture_stop_rx)
                .await
            {
                error!("Capture manager {instance} failed: {error}");
            }
        });
        Ok(Runtime {
            instance,
            tablet_tx: self.tablet,
            relaunch: self.relaunch,
            stop_tx,
            tasks: vec![stream, input, gate, stop],
            capture,
            video_port: self.ports.0,
            input_port: self.ports.1,
        })
    }
}

/// Own all session workers, including the gate and shutdown bridge. A startup
/// failure occurs before workers spawn; teardown waits for capture child reaping.
pub(crate) struct Runtime {
    pub instance: u32,
    pub tablet_tx: crate::attachment::Attachment,
    pub relaunch: Arc<Notify>,
    pub stop_tx: watch::Sender<bool>,
    pub tasks: Vec<JoinHandle<()>>,
    pub capture: JoinHandle<()>,
    pub video_port: u16,
    pub input_port: u16,
}

impl Runtime {
    pub async fn stop(mut self) {
        let _ = self.tablet_tx.send(false);
        let _ = self.stop_tx.send(true);
        if tokio::time::timeout(Duration::from_secs(5), &mut self.capture)
            .await
            .is_err()
        {
            warn!("Capture pipeline {} did not stop within 5s", self.instance);
            self.capture.abort();
            let _ = (&mut self.capture).await;
        }
        for task in &self.tasks {
            task.abort();
        }
        for task in &mut self.tasks {
            let _ = task.await;
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
        self.capture.abort();
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn forward_shutdown(
    mut daemon: watch::Receiver<bool>,
    mut local: watch::Receiver<bool>,
    capture: watch::Sender<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::select! {
            _ = daemon.changed() => {},
            _ = local.changed() => {},
        }
        let _ = capture.send(true);
    })
}

pub(crate) async fn start_servers(
    stream: stream::StreamServer,
    input: input::InputServer,
    video: crate::video_queue::VideoSender,
) -> Result<(JoinHandle<()>, JoinHandle<()>)> {
    let video_listener = stream.bind().await?;
    let input_listener = input.bind().await?;
    let stream = tokio::spawn(async move {
        if let Err(error) = stream.run_with_listener(video, video_listener).await {
            error!("Stream server failed: {error}");
        }
    });
    let input = tokio::spawn(async move {
        if let Err(error) = input.run_with_listener(input_listener).await {
            error!("Input server failed: {error}");
        }
    });
    Ok((stream, input))
}

fn spawn_display_gate(
    gate: watch::Sender<bool>,
    mut tablet: watch::Receiver<bool>,
    mut mode: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last = false;
        loop {
            let attached = *tablet.borrow();
            let active = attached && !*mode.borrow();
            if active != last {
                last = active;
                let _ = gate.send(active);
            }
            tokio::select! {
                r = tablet.changed() => if r.is_err() { break },
                r = mode.changed() => if r.is_err() { break },
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
