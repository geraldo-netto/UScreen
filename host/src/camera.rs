//! Opt-in webcam sidecar. Does not own or restart the display daemon/EVDI.
mod bridge;
mod decoder;
mod outputs;
mod protocol;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::{net::TcpListener, sync::watch, task::JoinSet};
use uscreen_config::camera::CameraOptions;

pub async fn run(options: &CameraOptions) -> Result<()> {
    options.validate()?;
    let devices = [
        outputs::open_device(&options.front_device, "UScreen Front")?,
        outputs::open_device(&options.rear_device, "UScreen Rear")?,
    ];
    let ffmpeg = executable("ffmpeg")?;
    let adb = executable("adb")?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let token = crate::runtime::random_token()?;
    let bridge = bridge::Bridge::create(
        adb,
        options.serial.as_deref(),
        listener.local_addr()?.port(),
    )
    .await?;
    let result = serve(&listener, &bridge, &token, &ffmpeg, options).await;
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
) -> Result<()> {
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
    let result = invited_session(
        listener,
        bridge,
        token,
        ffmpeg,
        options,
        &frames,
        &mut producers,
    )
    .await;
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

async fn invited_session(
    listener: &TcpListener,
    bridge: &bridge::Bridge,
    token: &str,
    ffmpeg: &std::path::Path,
    options: &CameraOptions,
    frames: &[outputs::Frames; 2],
    producers: &mut JoinSet<Result<()>>,
) -> Result<()> {
    bridge.invite(token, options).await?;
    tracing::info!("Webcams ready. Open UScreen settings on tablet and select Front or Rear. Ctrl-C stops sharing.");
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = sessions(listener, token, ffmpeg, options, frames) => result,
        result = tokio::signal::ctrl_c() => { result?; Ok(()) }
        _ = terminate.recv() => Ok(()),
        result = producers.join_next() => { result.context("camera producers stopped")???; anyhow::bail!("camera producer stopped") }
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
