//! Windows interactive daemon lifecycle; capture remains unsupported; direct input is an opt-in preview.
use anyhow::{bail, Result};
use blent::{
    tray_state::State,
    usb::{monitor::Monitor, Adb, NativeCommands},
};
use blent_config::{
    cli::{Cli, Commands},
    windows::{lifecycle, runtime},
};
use clap::Parser;
use std::{path::Path, time::Duration};
use tokio::sync::watch;

pub(super) fn run() -> Result<()> {
    let cli = Cli::parse();
    let direct_start = matches!(cli.command, None | Some(Commands::Start)) && !cli.login;
    cli.validate_backend_options(blent_config::platform::capabilities(), direct_start)?;
    blent_config::scheduling::apply_configured();
    let path = cli
        .runtime_dir
        .clone()
        .map(Ok)
        .unwrap_or_else(runtime::runtime_dir)?;
    if cli.login {
        anyhow::ensure!(
            cli.command.is_none(),
            "--login cannot be combined with a command"
        );
        return lifecycle::launch(&std::env::current_exe()?, &path, Duration::from_secs(5));
    }
    match &cli.command {
        None | Some(Commands::Start) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(serve(
                &path,
                cli.connection_settings(blent_config::FileConfig::load())?,
            )),
        Some(Commands::Wifi { off }) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(setup_wifi(
                *off,
                blent_config::windows::programs::find_in(
                    "adb",
                    &std::env::var_os("PATH").unwrap_or_default(),
                ),
                &blent_config::storage::ConfigStore::default(),
            )),
        Some(Commands::Stop) => lifecycle::stop(&path, lifecycle::STOP_TIMEOUT),
        Some(Commands::Status) => status(&path),
        Some(Commands::Doctor) => {
            diagnostics();
            Ok(())
        }
        _ => bail!("Display, input, camera and connection backends are unsupported on Windows"),
    }
}
async fn setup_wifi(
    off: bool,
    adb: Option<std::path::PathBuf>,
    store: &blent_config::storage::ConfigStore,
) -> Result<()> {
    anyhow::ensure!(
        off || adb.is_some(),
        "Wi-Fi setup unavailable: adb.exe missing from PATH"
    );
    let address = blent::wifi::setup(&Adb(adb.map(NativeCommands)), store, off).await?;
    println!(
        "{}",
        address.map_or_else(
            || "Wi-Fi off; saved address forgotten; tablet listener unchanged".into(),
            |address| format!("Connected to {address}; USB remains preferred")
        )
    );
    Ok(())
}
async fn serve(path: &Path, config: blent_config::FileConfig) -> Result<()> {
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
    let (status, state) = watch::channel(State::Starting);
    let updates = blent::update::Subscription::start(
        config.check_updates,
        blent_config::windows::paths::system()?
            .join("curl.exe")
            .into(),
    );
    let _tray = blent::windows_tray::Tray::start_with_updates(
        state,
        stop.clone(),
        updates.receiver.clone(),
    )
    .map_err(|error| eprintln!("Tray unavailable: {error:#}"))
    .ok();
    let result = run_usb(session, config, stop, receiver, status).await;
    updates.shutdown().await;
    result
}
async fn run_usb(
    session: &lifecycle::Session,
    config: blent_config::FileConfig,
    stop: watch::Sender<bool>,
    receiver: watch::Receiver<bool>,
    status: watch::Sender<State>,
) -> Result<()> {
    let monitor = usb_loop(session, config, receiver.clone(), &status);
    tokio::pin!(monitor);
    println!(
        "Blent daemon running; USB/Wi-Fi connection preview; display/stylus unsupported; touch/mouse preview opt-in on Windows"
    );
    let requested = tokio::select! {
        result=wait_stop(session, receiver) => result,
        result=&mut monitor => return result,
    };
    status.send_replace(State::Stopping);
    stop.send_replace(true);
    let cleanup = monitor.await;
    requested.and(cleanup)
}
async fn wait_stop(session: &lifecycle::Session, mut stop: watch::Receiver<bool>) -> Result<()> {
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            result=&mut shutdown => return result.map_err(Into::into),
            _=stop.wait_for(|value| *value) => return Ok(()),
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
    status: &watch::Sender<State>,
) -> Result<()> {
    let program = blent_config::windows::programs::find_in(
        "adb",
        &std::env::var_os("PATH").unwrap_or_default(),
    );
    let Some(program) = program else {
        println!("USB connection unavailable: adb.exe missing from PATH");
        status.send_replace(State::Unavailable);
        let _ = stop.wait_for(|stop| *stop).await;
        return Ok(());
    };
    let mut monitor = Monitor::new(Adb(NativeCommands(program)), config, stop.clone())?
        .with_network(blent_config::storage::ConfigStore::default());
    let result = poll_usb(session, &mut monitor, &mut stop, status).await;
    let cleanup = monitor.shutdown().await;
    result.and(cleanup)
}
async fn poll_usb(
    session: &lifecycle::Session,
    monitor: &mut Monitor<NativeCommands>,
    stop: &mut watch::Receiver<bool>,
    status: &watch::Sender<State>,
) -> Result<()> {
    let mut previous = Vec::new();
    session.publish_sessions(&previous)?;
    status.send_replace(State::Waiting);
    loop {
        monitor.poll().await;
        let sessions = monitor.sessions();
        if sessions != previous {
            session.publish_sessions(&sessions)?;
            status.send_replace(State::connections(&sessions));
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
            "Blent daemon running (PID {}); display/stylus unsupported; touch/mouse preview opt-in",
            owner.pid
        ),
        None => {
            println!("Blent daemon stopped; display/stylus unsupported; touch/mouse preview opt-in")
        }
    }
    let sessions = lifecycle::load_sessions(path).unwrap_or_default();
    for session in sessions {
        println!(
            "{} prepared: {} (slot {}, ports {}/{})",
            blent_config::adb::transport_of(&session.serial).label(),
            session.serial,
            session.instance,
            session.video_port,
            session.input_port
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
    print_input_monitors();
    for line in blent_config::diagnostics::collect().lines() {
        println!("{line}");
    }
}

#[cfg(test)]
#[path = "windows_wifi_tests.rs"]
mod wifi_tests;

fn print_input_monitors() {
    use blent_config::input_mapping::MonitorInventory;
    match blent_config::windows::monitors::NativeInventory.snapshot() {
        Ok(snapshot) => {
            for monitor in snapshot.monitors() {
                println!("Input monitor: {} ({})", monitor.id, monitor.name);
            }
        }
        Err(error) => println!("Input monitors unavailable: {error}"),
    }
}
