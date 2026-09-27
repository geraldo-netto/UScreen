//! Windows interactive daemon lifecycle; capture and input remain unsupported.
use anyhow::{bail, Result};
use blent::usb::{monitor::Monitor, Adb, NativeCommands};
use blent_config::{
    cli::{Cli, Commands},
    windows::{lifecycle, runtime},
};
use clap::Parser;
use std::{path::Path, time::Duration};
use tokio::sync::watch;

pub(super) fn run() -> Result<()> {
    let cli = Cli::parse();
    blent_config::scheduling::apply_configured();
    let path = cli
        .runtime_dir
        .map(Ok)
        .unwrap_or_else(runtime::runtime_dir)?;
    if cli.login {
        anyhow::ensure!(
            cli.command.is_none(),
            "--login cannot be combined with a command"
        );
        return lifecycle::launch(&std::env::current_exe()?, &path, Duration::from_secs(5));
    }
    match cli.command {
        None | Some(Commands::Start) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(serve(&path, cli.video_port, cli.input_port)),
        Some(Commands::Stop) => lifecycle::stop(&path, lifecycle::STOP_TIMEOUT),
        Some(Commands::Status) => status(&path),
        Some(Commands::Doctor) => {
            diagnostics();
            Ok(())
        }
        _ => bail!("Display, input, camera and connection backends are unsupported on Windows"),
    }
}
async fn serve(path: &Path, video: Option<u16>, input: Option<u16>) -> Result<()> {
    let mut config = blent_config::FileConfig::load();
    config.video_port = video.unwrap_or(config.video_port);
    config.input_port = input.unwrap_or(config.input_port);
    blent_config::slot_ports(config.video_port, config.input_port, config.max_tablets)?;
    anyhow::ensure!(
        config.require_token,
        "Windows USB preview requires require_token = true"
    );
    let session = lifecycle::Session::start(path)?;
    let result = serve_usb(&session, config).await;
    result.and(session.shutdown())
}
async fn serve_usb(session: &lifecycle::Session, config: blent_config::FileConfig) -> Result<()> {
    let (stop, receiver) = watch::channel(false);
    let monitor = usb_loop(session, config, receiver);
    tokio::pin!(monitor);
    println!(
        "Blent daemon running; USB connection preview; display and input unsupported on Windows"
    );
    let requested = tokio::select! {
        result=wait_stop(session) => result,
        result=&mut monitor => return result,
    };
    stop.send_replace(true);
    let cleanup = monitor.await;
    requested.and(cleanup)
}
async fn wait_stop(session: &lifecycle::Session) -> Result<()> {
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            result=&mut shutdown => return result.map_err(Into::into),
            _=tokio::time::sleep(Duration::from_millis(50)) => {
                if session.stop_requested() { return Ok(()); }
            }
        }
    }
}
async fn usb_loop(
    session: &lifecycle::Session,
    config: blent_config::FileConfig,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    let program = blent_config::windows::programs::find_in(
        "adb",
        &std::env::var_os("PATH").unwrap_or_default(),
    );
    let Some(program) = program else {
        println!("USB connection unavailable: adb.exe missing from PATH");
        let _ = stop.wait_for(|stop| *stop).await;
        return Ok(());
    };
    let mut monitor = Monitor::new(Adb(NativeCommands(program)), config, stop.clone())?;
    let result = poll_usb(session, &mut monitor, &mut stop).await;
    let cleanup = monitor.shutdown().await;
    result.and(cleanup)
}
async fn poll_usb(
    session: &lifecycle::Session,
    monitor: &mut Monitor<NativeCommands>,
    stop: &mut watch::Receiver<bool>,
) -> Result<()> {
    let mut previous = Vec::new();
    session.publish_sessions(&previous)?;
    loop {
        monitor.poll().await;
        let sessions = monitor.sessions();
        if sessions != previous {
            session.publish_sessions(&sessions)?;
            previous = sessions;
        }
        tokio::select! {
            _=stop.wait_for(|stop| *stop) => return Ok(()),
            _=tokio::time::sleep(Duration::from_secs(1)) => {},
        }
    }
}

fn status(path: &Path) -> Result<()> {
    match lifecycle::status(path)? {
        Some(owner) => println!(
            "Blent daemon running (PID {}); display and input unsupported",
            owner.pid
        ),
        None => println!("Blent daemon stopped; display and input unsupported"),
    }
    let sessions = lifecycle::load_sessions(path).unwrap_or_default();
    for session in sessions {
        println!(
            "USB prepared: {} (slot {}, ports {}/{})",
            session.serial, session.instance, session.video_port, session.input_port
        );
    }
    Ok(())
}
fn diagnostics() {
    println!("Blent Windows build preview");
    match blent_config::config_path() {
        Ok(path) => println!("Configuration: {}", path.display()),
        Err(error) => println!("Configuration: unavailable ({error})"),
    }
    println!("Blent version: {}", env!("CARGO_PKG_VERSION"));
    for line in blent_config::diagnostics::collect().lines() {
        println!("{line}");
    }
}
