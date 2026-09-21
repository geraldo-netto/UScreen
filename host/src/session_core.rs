//! Portable session ownership. Native capture and input arrive as explicit adapters.
use crate::{attachment::Attachment, input, latency::LatencyTracker, media, stream, video_queue};
use anyhow::Result;
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;
use tracing::{error, warn};

#[derive(Clone, Default)]
pub struct CaptureResources {
    pub codec_config: media::CodecConfig,
    pub idr_wanted: Arc<AtomicBool>,
    pub latency: LatencyTracker,
}

/// No native resources may be started until session listeners have bound.
pub trait CaptureBackend: Send {
    fn resources(&self) -> CaptureResources;
    fn start(self: Box<Self>, context: CaptureContext) -> CaptureWorkers;
}

pub struct CaptureContext {
    pub video: video_queue::VideoSender,
    pub settings: watch::Sender<media::EncoderSettings>,
    pub settings_rx: watch::Receiver<media::EncoderSettings>,
    pub display: watch::Receiver<bool>,
    pub stop: watch::Receiver<bool>,
    pub attachment: Attachment,
}

/// All native workers remain owned by the session and joined on shutdown.
pub struct CaptureWorkers {
    pub capture: JoinHandle<()>,
    pub auxiliary: Vec<JoinHandle<()>>,
}

pub struct Spec {
    pub settings: media::EncoderSettings,
    pub instance: u32,
    pub ports: (u16, u16),
    pub token: Option<String>,
    pub devices: (bool, bool, bool),
}

impl Spec {
    fn input_config(&self) -> input::InputConfig {
        input::InputConfig {
            port: self.ports.1,
            instance: self.instance,
            token: self.token.clone(),
            codec: media::Codec::from_encoder(&self.settings.encoder)
                .wire_name()
                .into(),
            virtual_width: self.settings.width,
            virtual_height: self.settings.height,
            touch: self.devices.0,
            pen: self.devices.1,
            pointer: self.devices.2,
        }
    }

    /// Prepare state without opening sockets or starting native workers.
    pub fn prepare(
        self,
        mode: watch::Sender<bool>,
        capture: Box<dyn CaptureBackend>,
        backend: Arc<dyn input::backend::InputBackend>,
    ) -> Prepared {
        let config = self.input_config();
        let resources = capture.resources();
        let (settings, settings_rx) = watch::channel(self.settings);
        let tablet = Attachment::with_token(settings.clone(), self.token.clone());
        let relaunch = Arc::new(Notify::new());
        let stream = stream::StreamServer::new(
            stream::StreamConfig {
                video_port: self.ports.0,
                token: self.token,
            },
            resources.codec_config.clone(),
            resources.idr_wanted.clone(),
        )
        .with_attachment(tablet.clone());
        let input = input::InputServer::with_backend(
            config,
            Some(settings.clone()),
            mode.clone(),
            resources.latency.clone(),
            relaunch.clone(),
            tablet.subscribe(),
            backend,
        )
        .with_attachment(tablet.clone());
        Prepared {
            settings,
            settings_rx,
            tablet,
            relaunch,
            mode,
            capture,
            resources,
            stream,
            input,
            instance: self.instance,
            ports: self.ports,
        }
    }
}

pub struct Prepared {
    pub settings: watch::Sender<media::EncoderSettings>,
    pub settings_rx: watch::Receiver<media::EncoderSettings>,
    pub tablet: Attachment,
    pub relaunch: Arc<Notify>,
    pub resources: CaptureResources,
    pub stream: stream::StreamServer,
    pub input: input::InputServer,
    mode: watch::Sender<bool>,
    capture: Box<dyn CaptureBackend>,
    instance: u32,
    ports: (u16, u16),
}

impl Prepared {
    pub async fn start(self, daemon_stop: watch::Receiver<bool>) -> Result<Runtime> {
        let (video, _) =
            video_queue::channel(video_queue::QUEUE_PACKETS, self.resources.idr_wanted);
        let (stream, input) = start_servers(self.stream, self.input, video.clone()).await?;
        let (gate_tx, gate_rx) = watch::channel(false);
        let (stop_tx, stop_rx) = watch::channel(false);
        let (capture_stop, capture_stop_rx) = watch::channel(false);
        let gate = spawn_display_gate(gate_tx, self.tablet.subscribe(), self.mode.subscribe());
        let stop = forward_shutdown(daemon_stop, stop_rx, capture_stop);
        let workers = self.capture.start(CaptureContext {
            video,
            settings: self.settings,
            settings_rx: self.settings_rx,
            display: gate_rx,
            stop: capture_stop_rx,
            attachment: self.tablet.clone(),
        });
        let mut tasks = vec![stream, input, gate, stop];
        tasks.extend(workers.auxiliary);
        Ok(Runtime {
            instance: self.instance,
            tablet_tx: self.tablet,
            relaunch: self.relaunch,
            stop_tx,
            tasks,
            capture: workers.capture,
            video_port: self.ports.0,
            input_port: self.ports.1,
        })
    }
}

/// Own all session workers, including the gate and shutdown bridge. A startup
/// failure occurs before workers spawn; teardown waits for capture child reaping.
pub struct Runtime {
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

pub fn forward_shutdown(
    mut daemon: watch::Receiver<bool>,
    mut local: watch::Receiver<bool>,
    capture: watch::Sender<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::select! {
            _ = daemon.wait_for(|stop| *stop) => {},
            _ = local.wait_for(|stop| *stop) => {},
        }
        let _ = capture.send(true);
    })
}

pub async fn start_servers(
    stream: stream::StreamServer,
    input: input::InputServer,
    video: crate::video_queue::VideoSender,
) -> Result<(JoinHandle<()>, JoinHandle<()>)> {
    let video_listener = stream.bind().await?;
    let input_listener = input.bind().await?;
    let stream = spawn_server("Stream", async move {
        stream.run_with_listener(video, video_listener).await
    });
    let input = spawn_server("Input", async move {
        input.run_with_listener(input_listener).await
    });
    Ok((stream, input))
}

pub fn spawn_server(
    name: &'static str,
    run: impl std::future::Future<Output = Result<()>> + Send + 'static,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(error) = run.await {
            error!("{name} server failed: {error}");
        }
    })
}

pub fn spawn_display_gate(
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
