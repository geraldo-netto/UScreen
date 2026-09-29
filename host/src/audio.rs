//! Linux audio transport and isolated PipeWire adapter.
use crate::{audio_control::Report, camera::bridge::Bridge, camera_control::cancelled};
use anyhow::{ensure, Context, Result};
use blent_config::audio::{AudioOptions, AudioSession, AudioState, Direction};
use blent_config::commands::AsyncCommandExt;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    process::Command,
    sync::watch,
};
mod process;
mod protocol;

pub async fn run(
    options: AudioOptions,
    mut stop: watch::Receiver<bool>,
    report: Report,
) -> Result<()> {
    options.profile.validate()?;
    let adb = executable("adb")?;
    let helper = helper()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let bridge = Bridge::create(
        adb,
        options.serial.as_deref(),
        listener.local_addr()?.port(),
    )
    .await?;
    let result = invited(&listener, &bridge, &helper, &options, &report, &mut stop).await;
    let cleanup = bridge.close().await;
    result.and(cleanup)
}

fn executable(name: &str) -> Result<PathBuf> {
    blent_config::linux::programs::find_in(name, &std::env::var_os("PATH").unwrap_or_default())
        .with_context(|| format!("Audio requires {name}"))
}
fn helper() -> Result<PathBuf> {
    let current = std::env::current_exe()?;
    let sibling = current
        .parent()
        .context("missing executable directory")?
        .join("blent-audio");
    if sibling.is_file() {
        return Ok(sibling);
    }
    executable("blent-audio")
}

async fn invited(
    listener: &TcpListener,
    bridge: &Bridge,
    helper: &Path,
    options: &AudioOptions,
    report: &Report,
    stop: &mut watch::Receiver<bool>,
) -> Result<()> {
    let token = blent_config::credentials::random_token()?;
    let setup = async {
        invite(bridge, &token, options).await?;
        protocol::accept(listener, &token, options.profile.direction).await
    };
    let (mut socket, capabilities, mode, processing) = tokio::select! {
        result = setup => result?,
        _ = cancelled(stop) => return Ok(()),
    };
    let mut session = AudioSession::new(options.profile.direction);
    let mut profile = options.profile;
    profile.processing = mode;
    let grant = session.start(profile, capabilities, true, 0)?;
    let mut child = process::spawn(helper, options.profile, &token)?;
    let started = std::time::Instant::now();
    let stream = async {
        process::ready(&mut child)
            .await
            .context("PipeWire unavailable or virtual audio device setup failed")?;
        protocol::grant(&mut socket, &token, &grant).await?;
        session.connected(
            grant.generation(),
            &grant.hello(),
            started.elapsed().as_millis() as u64,
        )?;
        report.update(
            AudioState::Streaming,
            format!("Blent {:?} available. {processing}", profile.direction),
        );
        match profile.direction {
            Direction::Microphone => {
                let mut reader = grant.authenticate(&grant.hello())?;
                microphone(&mut socket, &mut child, &mut reader).await
            }
            Direction::Speakers => speakers(&mut socket, &mut child, grant).await,
        }
    };
    let result = tokio::select! {
        result = stream => result,
        _ = cancelled(stop) => Ok(()),
    };
    process::retire(&mut child).await;
    result
}

async fn invite(bridge: &Bridge, token: &str, options: &AudioOptions) -> Result<()> {
    let port: u16 = bridge
        .remote
        .strip_prefix("tcp:")
        .context("invalid ADB audio port")?
        .parse()?;
    let profile = options.profile;
    let command = format!("am broadcast -n io.github.geraldo_netto.blent/com.blent.AudioReceiver --es token {token} --ei port {port} --ei direction {} --ei processing {} --ei buffer_ms {} --ez background {}\n", profile.direction as u8, profile.processing as u8, profile.buffer_ms, profile.background);
    let result = Command::new(&bridge.adb)
        .args(["-s", &bridge.serial, "shell"])
        .output_input_timeout(Some(command.as_bytes()), Duration::from_secs(5))
        .await?;
    ensure!(
        result.status.success() && String::from_utf8_lossy(&result.stdout).contains("result=1"),
        "Tablet did not accept audio request. Install updated Blent and open it."
    );
    Ok(())
}

async fn microphone(
    socket: &mut TcpStream,
    child: &mut tokio::process::Child,
    reader: &mut blent_config::audio::FrameReader,
) -> Result<()> {
    let input = child.stdin.as_mut().context("missing audio helper input")?;
    loop {
        let packet =
            tokio::time::timeout(Duration::from_millis(250), protocol::packet(socket, reader))
                .await??;
        tokio::time::timeout(Duration::from_millis(250), input.write_all(&packet)).await??;
    }
}

async fn speakers(
    socket: &mut TcpStream,
    child: &mut tokio::process::Child,
    grant: blent_config::audio::AudioGrant,
) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let output = child
        .stdout
        .as_mut()
        .context("missing audio helper output")?;
    let mut writer = blent_config::audio::FrameWriter::new(grant);
    let epoch = std::time::Instant::now();
    loop {
        let mut bytes = [0; 1921];
        tokio::time::timeout(Duration::from_millis(250), output.read_exact(&mut bytes)).await??;
        let block = protocol::captured(&bytes)?;
        let packet = writer.encode(&block, epoch.elapsed().as_micros() as u64)?;
        tokio::time::timeout(Duration::from_millis(250), socket.write_all(&packet)).await??;
    }
}

/// CLI shares the exact GUI pipeline and retires only its owned direction.
pub async fn run_cli(options: AudioOptions) -> Result<()> {
    let (stop, stopped) = watch::channel(false);
    let (report, _) = watch::channel(Default::default());
    let work = run(options, stopped, Report(report));
    tokio::pin!(work);
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = &mut work => result,
        result = tokio::signal::ctrl_c() => { result?; stop.send_replace(true); work.await },
        _ = terminate.recv() => { stop.send_replace(true); work.await },
    }
}

#[cfg(test)]
mod tests;
