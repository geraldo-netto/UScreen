//! Host-owned webcam pipeline; independent of the display daemon/EVDI.
use crate::camera_control::{self as control, Report};
use uscreen_config::camera::CameraState as State;
mod bridge;
mod decoder;
mod outputs;
mod protocol;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::{net::TcpListener, sync::watch, task::JoinSet};
use uscreen_config::camera::{CameraOptions, CameraProfile};

pub async fn run(options: &CameraOptions) -> Result<()> {
    let (stop, stopped) = watch::channel(false);
    let (status, _) = watch::channel(State::Starting);
    let status = Report {
        state: status,
        preview: watch::channel(None).0,
    };
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let work = run_native(options.clone(), stopped, status);
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => result,
        signal = tokio::signal::ctrl_c() => { signal?; stop.send_replace(true); work.await }
        _ = terminate.recv() => { stop.send_replace(true); work.await }
    }
}

pub async fn run_controlled(
    profile: CameraProfile,
    stop: watch::Receiver<bool>,
    status: Report,
) -> Result<()> {
    run_native(
        CameraOptions {
            profile,
            ..Default::default()
        },
        stop,
        status,
    )
    .await
}

async fn run_native(
    options: CameraOptions,
    mut stop: watch::Receiver<bool>,
    status: Report,
) -> Result<()> {
    options.validate()?;
    let devices = [
        outputs::open_device(&options.front_device, "UScreen Front")?,
        outputs::open_device(&options.rear_device, "UScreen Rear")?,
    ];
    let ffmpeg = executable("ffmpeg")?;
    let adb = executable("adb")?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let token = uscreen_config::runtime::random_token()?;
    let bridge = bridge::Bridge::create(
        adb,
        options.serial.as_deref(),
        listener.local_addr()?.port(),
    )
    .await?;
    let result = serve(
        &listener, &bridge, &token, &ffmpeg, &options, &mut stop, &status,
    )
    .await;
    if let Err(error) = bridge.close().await {
        tracing::warn!(%error, "camera mapping cleanup failed");
    }
    drop(devices);
    result
}

fn executable(name: &str) -> Result<PathBuf> {
    uscreen_config::linux::programs::find_in(name, &std::env::var_os("PATH").unwrap_or_default())
        .with_context(|| format!("camera sharing requires {name}"))
}

async fn serve(
    listener: &TcpListener,
    bridge: &bridge::Bridge,
    token: &str,
    ffmpeg: &std::path::Path,
    options: &CameraOptions,
    stop: &mut watch::Receiver<bool>,
    status: &Report,
) -> Result<()> {
    if *stop.borrow() {
        return Ok(());
    }
    let (front, front_rx) = watch::channel(None);
    let (rear, rear_rx) = watch::channel(None);
    let mut producers = JoinSet::new();
    for (device, frames) in [
        (&options.front_device, front_rx),
        (&options.rear_device, rear_rx),
    ] {
        let mut child = outputs::producer(ffmpeg, device, options)
            .spawn()
            .context("start virtual camera producer")?;
        let options = options.clone();
        producers.spawn(async move {
            let result = outputs::feed(&mut child, frames, &options).await;
            outputs::retire(&mut child).await;
            result
        });
    }
    let frames = [front, rear];
    let session = Session {
        listener,
        bridge,
        token,
        ffmpeg,
        options,
    };
    let result = session.invited(&frames, &mut producers, stop, status).await;
    drop(frames);
    if tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while producers.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        producers.shutdown().await;
    }
    result
}

struct Session<'a> {
    listener: &'a TcpListener,
    bridge: &'a bridge::Bridge,
    token: &'a str,
    ffmpeg: &'a std::path::Path,
    options: &'a CameraOptions,
}

impl Session<'_> {
    async fn invited(
        &self,
        frames: &[outputs::Frames; 2],
        producers: &mut JoinSet<Result<()>>,
        stop: &mut watch::Receiver<bool>,
        status: &Report,
    ) -> Result<()> {
        self.bridge.invite(self.token, self.options).await?;
        status.update(State::Waiting);
        tracing::info!("Webcams ready; waiting for tablet permission and camera frames.");
        tokio::select! {
            result = sessions(self.listener, self.token, self.ffmpeg, self.options, frames) => result,
            _ = control::cancelled(stop) => Ok(()),
            _ = frame_status(frames, self.options, status) => Ok(()),
            result = producers.join_next() => { result.context("camera producers stopped")???; anyhow::bail!("camera producer stopped") }
        }
    }
}

async fn sessions(
    listener: &TcpListener,
    token: &str,
    ffmpeg: &std::path::Path,
    options: &CameraOptions,
    frames: &[outputs::Frames; 2],
) -> Result<()> {
    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        if let Err(error) = decoder::connection(socket, token, ffmpeg, options, frames).await {
            tracing::info!(%error, "camera connection ended; waiting for tablet selection");
        }
    }
}

async fn frame_status(frames: &[outputs::Frames; 2], options: &CameraOptions, status: &Report) {
    let mut selected = frames[match options.lens {
        uscreen_config::camera::Lens::Front => 0,
        uscreen_config::camera::Lens::Rear => 1,
    }]
    .subscribe();
    let mut next_preview = tokio::time::Instant::now();
    loop {
        if selected.changed().await.is_err() {
            return;
        }
        let frame = selected.borrow_and_update().clone();
        let Some(frame) = frame else {
            status.update(State::Waiting);
            continue;
        };
        status.update(State::Streaming);
        if tokio::time::Instant::now() >= next_preview {
            let preview = uscreen_config::camera::CameraPreview::from_yuv420(
                options.lens,
                options.width,
                options.height,
                &frame,
            )
            .ok()
            .map(std::sync::Arc::new);
            status.preview.send_replace(preview);
            next_preview = tokio::time::Instant::now() + std::time::Duration::from_millis(200);
        }
    }
}
