//! Opt-in native benchmark: real tablet → production decoder, no V4L2 required.
//! Outputs timing/counts only; never stores camera pictures. Stops capture on exit.
#![allow(dead_code)]
#[path = "../src/camera/bridge.rs"]
mod bridge;
#[path = "../src/camera/decoder.rs"]
mod decoder;
#[path = "../src/camera/outputs.rs"]
mod outputs;
#[path = "../src/camera/protocol.rs"]
mod protocol;

use anyhow::{Context, Result};
use blent_config::{camera::CameraOptions, commands::AsyncCommandExt};
use clap::Parser;
use std::{path::Path, time::Duration};
use tokio::{net::TcpListener, process::Command, sync::watch, time::Instant};

#[derive(Parser)]
struct Options {
    #[command(flatten)]
    camera: CameraOptions,
    #[arg(long, default_value_t = 10)]
    seconds: u64,
    #[arg(long, default_value_t = 0)]
    stall_feedback_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Options::parse();
    args.camera.validate()?;
    anyhow::ensure!(
        (1..=30).contains(&args.seconds),
        "probe duration must be 1–30 seconds"
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let route = TcpListener::bind("127.0.0.1:0").await?;
    let bridge = bridge::Bridge::create(
        "/usr/bin/adb".into(),
        args.camera.serial.as_deref(),
        route.local_addr()?.port(),
    )
    .await?;
    let token = blent_config::runtime::random_token()?;
    let result = tokio::select! {
        result = sample(&listener, &bridge, &token, &args) => result,
        result = proxy(&route, listener.local_addr()?, args.stall_feedback_ms) => result,
    };
    let stop = stop_capture(&bridge).await;
    let cleanup = bridge.close().await;
    result?;
    stop?;
    cleanup
}

async fn stop_capture(bridge: &bridge::Bridge) -> Result<()> {
    // A valid invitation without a requested lens retires capture; no app stop,
    // display restart or borrowed mapping deletion is required.
    let token = blent_config::runtime::random_token()?;
    let command = format!("am broadcast -n io.github.geraldo_netto.blent/com.blent.CameraReceiver --es token {token} --ei port 1 --ei width 1280 --ei height 720 --ei fps 30 --ei bitrate 3000\n");
    let result = Command::new(&bridge.adb)
        .args(["-s", &bridge.serial, "shell"])
        .output_input_timeout(Some(command.as_bytes()), Duration::from_secs(5))
        .await?;
    anyhow::ensure!(result.status.success(), "probe camera stop failed");
    Ok(())
}

async fn sample(
    listener: &TcpListener,
    bridge: &bridge::Bridge,
    token: &str,
    args: &Options,
) -> Result<()> {
    let (front, mut received) = watch::channel(None);
    let (rear, _) = watch::channel(None);
    let frames = [front, rear];
    if matches!(args.camera.lens, blent_config::camera::Lens::Rear) {
        received = frames[1].subscribe();
    }
    bridge.invite(token, &args.camera).await?;
    let started = Instant::now();
    let mut count = 0u64;
    let mut decoded_ms = Vec::with_capacity((args.seconds * 30) as usize);
    let mut first_ms = None;
    let accept = async {
        loop {
            let (socket, _) = listener.accept().await?;
            let result = decoder::connection(
                socket,
                token,
                Path::new("/usr/bin/ffmpeg"),
                &args.camera,
                &frames,
            )
            .await;
            eprintln!("camera generation ended: {result:?}");
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    };
    let observe = async {
        loop {
            received.changed().await.context("probe output closed")?;
            if received.borrow_and_update().is_some() {
                count += 1;
                decoded_ms.push(started.elapsed().as_millis());
                first_ms.get_or_insert(started.elapsed().as_millis());
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    };
    tokio::select! {
        result = accept => result?,
        result = observe => result?,
        _ = tokio::time::sleep(Duration::from_secs(args.seconds)) => {}
    }
    println!(
        "{}",
        serde_json::json!({"seconds": args.seconds, "frames": count, "decoded_ms": decoded_ms, "stall_feedback_ms": args.stall_feedback_ms, "first_frame_ms": first_ms,
        "bitrate_kbps": args.camera.bitrate, "adaptive": args.camera.adaptive_bitrate,
        "freshness_ms": args.camera.freshness_ms, "boundary": "camera sensor through production decoder; no V4L2/presentation"})
    );
    anyhow::ensure!(count > 0, "no native camera frames decoded");
    Ok(())
}

async fn proxy(listener: &TcpListener, target: std::net::SocketAddr, stall_ms: u64) -> Result<()> {
    anyhow::ensure!(stall_ms <= 1000, "stall must be ≤1000 ms");
    let started = Instant::now();
    let mut stalled = stall_ms == 0;
    loop {
        let (mut tablet, _) = listener.accept().await?;
        let mut host = tokio::net::TcpStream::connect(target).await?;
        tablet.set_nodelay(true)?;
        host.set_nodelay(true)?;
        let (mut tablet_in, mut tablet_out) = tablet.split();
        let (mut host_in, mut host_out) = host.split();
        tokio::select! {
            _ = tokio::io::copy(&mut tablet_in, &mut host_out) => {},
            _ = feedback(&mut host_in, &mut tablet_out, &mut stalled, started, stall_ms) => {},
        }
    }
}

async fn feedback(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    stalled: &mut bool,
    started: Instant,
    stall_ms: u64,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut bytes = [0; 8];
    loop {
        let size = reader.read(&mut bytes).await?;
        if size == 0 {
            return Ok(());
        }
        if !*stalled && started.elapsed() >= Duration::from_secs(2) {
            *stalled = true;
            eprintln!(
                "inject feedback stall at {} ms",
                started.elapsed().as_millis()
            );
            tokio::time::sleep(Duration::from_millis(stall_ms)).await;
        }
        writer.write_all(&bytes[..size]).await?;
    }
}
