mod capture;
mod config;
mod doctor;
mod edid;
#[cfg(feature = "inproc-encoder")]
mod encoder;
#[cfg_attr(not(feature = "inproc-encoder"), allow(dead_code))]
mod encoder_io;
mod input;
mod kscreen;
mod kwin;
mod latency;
mod osk;
mod runtime;
mod stream;
mod tray;
mod update;
mod vdisplay;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio::signal;
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};
use uscreen_config::commands::AsyncCommandExt;

#[derive(Parser)]
#[command(
    name = "uscreen",
    version,
    about = "USB second-screen server for Linux"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Explicit EDID override. By default an EDID is generated at runtime
    /// for the configured (or tablet-reported) resolution.
    #[arg(long = "edid")]
    edid: Option<PathBuf>,

    #[arg(long = "helper")]
    helper: Option<PathBuf>,

    /// Defaults come from ~/.config/uscreen/config.toml; CLI flags override.
    #[arg(long = "encoder")]
    encoder: Option<String>,

    #[arg(long = "fps")]
    fps: Option<u32>,

    #[arg(long = "bitrate")]
    bitrate: Option<u32>,

    #[arg(long = "width")]
    width: Option<u32>,

    #[arg(long = "height")]
    height: Option<u32>,

    #[arg(long = "quality")]
    quality: Option<u32>,

    /// Integer downscale for the stream only; the desktop keeps its native mode.
    #[arg(long = "stream-scale")]
    stream_scale: Option<u32>,

    /// Drive the laptop's own screen with the pen instead of streaming a second
    /// display to the tablet.
    #[arg(long = "pen-only")]
    pen_only: bool,

    #[arg(long = "video-port")]
    video_port: Option<u16>,

    #[arg(long = "input-port")]
    input_port: Option<u16>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the uscreen daemon
    Start,
    /// Stop the uscreen daemon
    Stop,
    /// Show daemon status
    Status,
    /// List available displays
    ListDisplays,
    /// Set the tablet up to connect over Wi-Fi, so the cable becomes optional
    Wifi {
        /// Forget the remembered address and stop reconnecting
        #[arg(long = "off")]
        off: bool,
    },
    /// Diagnose the whole setup and report what is wrong
    Doctor,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    setup_logging();

    match &cli.command {
        Some(Commands::Start) | None => {
            info!("Starting uscreen daemon");
            run_daemon(cli).await?;
        }
        Some(Commands::Stop) => stop_daemon().await?,
        Some(Commands::Status) => show_status().await?,
        Some(Commands::ListDisplays) => list_displays().await?,
        Some(Commands::Wifi { off }) => setup_wifi(*off).await?,
        Some(Commands::Doctor) => doctor::run().await?,
    }

    Ok(())
}

fn setup_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "uscreen=info".into()),
        )
        .with_target(true)
        .with_line_number(true)
        .init();
}

fn effective_config(cli: &Cli, saved: &config::FileConfig) -> config::FileConfig {
    let mut effective = config::FileConfig {
        encoder: cli.encoder.as_ref().unwrap_or(&saved.encoder).clone(),
        fps: cli.fps.unwrap_or(saved.fps),
        bitrate: cli.bitrate.unwrap_or(saved.bitrate),
        width: cli.width.unwrap_or(saved.width),
        height: cli.height.unwrap_or(saved.height),
        quality: cli.quality.unwrap_or(saved.quality),
        stream_scale: cli.stream_scale.unwrap_or(saved.stream_scale),
        video_port: cli.video_port.unwrap_or(saved.video_port),
        input_port: cli.input_port.unwrap_or(saved.input_port),
        ..saved.clone()
    };
    effective.sanitize();
    effective
}

#[cfg(test)]
mod cli_tests {
    #[test]
    fn t200_runtime_persistence_preserves_cli_and_unchanged_fields() {
        let cli =
            Cli::try_parse_from(["uscreen", "--encoder", "h264_vaapi", "--width", "1280"]).unwrap();
        let overrides = CliOverrides::new(&cli);
        let previous = capture::EncoderSettings {
            encoder: "h264_vaapi".into(),
            fps: 90,
            bitrate: 10000,
            width: 1280,
            height: 1080,
            quality: 20,
            width_mm: 310,
            height_mm: 194,
            stream_scale: 1,
            geometry_ready: true,
        };
        let changed = capture::EncoderSettings {
            encoder: "h264_nvenc".into(),
            bitrate: 12000,
            width: 2560,
            height: 1200,
            quality: 25,
            stream_scale: 2,
            ..previous.clone()
        };
        let saved = config::FileConfig {
            encoder: "libx264".into(),
            width: 1920,
            fps: 60,
            pen_only: true,
            ..Default::default()
        };
        let mut updated = saved.clone();
        overrides.apply_encoder(&mut updated, &changed, &previous);
        overrides.apply_geometry(&mut updated, &changed, &previous);
        assert_eq!(
            (updated.encoder.as_str(), updated.width, updated.fps),
            ("libx264", 1920, 60)
        );
        assert_eq!(
            (
                updated.bitrate,
                updated.height,
                updated.quality,
                updated.stream_scale
            ),
            (12000, 1200, 25, 2)
        );
        assert!(updated.pen_only);

        let mut unchanged = saved.clone();
        overrides.apply_encoder(&mut unchanged, &changed, &changed);
        overrides.apply_geometry(&mut unchanged, &changed, &changed);
        assert_eq!(
            unchanged, saved,
            "unchanged stream fields must not overwrite newer disk settings"
        );
    }

    #[test]
    fn t108_extra_slot_requires_its_own_assigned_card() {
        let mut config = capture::CaptureConfig::default();
        assert!(assign_slot_card(&mut config, &[0], 1).is_err());
        assign_slot_card(&mut config, &[0, 3], 1).unwrap();
        assert_eq!(config.card, Some(3));
        assign_slot_card(&mut config, &[0, 3], 0).unwrap();
        assert_eq!(config.card, Some(0));
    }

    #[test]
    fn t122_cli_distinguishes_omitted_helper_from_explicit_override() {
        assert!(Cli::try_parse_from(["uscreen"]).unwrap().helper.is_none());
        assert_eq!(
            Cli::try_parse_from(["uscreen", "--helper", "/custom/helper"])
                .unwrap()
                .helper,
            Some(PathBuf::from("/custom/helper"))
        );
    }

    #[test]
    fn t122_explicit_missing_helper_cannot_fall_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("installed");
        let explicit = dir.path().join("explicit");
        std::fs::write(&installed, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o700)).unwrap();
        let candidates = vec![installed.clone()];
        assert!(select_helper(Some(&explicit), &candidates).is_err());
        assert_eq!(select_helper(None, &candidates).unwrap(), installed);
        std::fs::write(&explicit, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&explicit, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            select_helper(Some(&explicit), &candidates).unwrap(),
            explicit
        );
        assert!(select_helper(Some(dir.path()), &candidates).is_err());
    }

    #[tokio::test]
    async fn t110_extra_token_delivery_is_targeted_and_rate_limited() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let adb = dir.path().join("adb");
        std::fs::write(
            &adb,
            "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' \"$2\" >> \"$0.log\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut policies = std::collections::HashMap::from([
            ("tablet-A".into(), RelaunchBackoff::default()),
            ("tablet-B".into(), RelaunchBackoff::default()),
        ]);
        let now = std::time::Instant::now();
        for _ in 0..10 {
            deliver_extra_token(
                "tablet-A",
                Some("deadbeef"),
                &mut policies,
                now,
                adb.to_str().unwrap(),
            )
            .await;
        }
        assert_eq!(
            std::fs::read_to_string(adb.with_extension("log")).unwrap(),
            "tablet-A\n"
        );
        deliver_extra_token(
            "tablet-A",
            None,
            &mut policies,
            now + std::time::Duration::from_secs(5),
            adb.to_str().unwrap(),
        )
        .await;
        deliver_extra_token(
            "tablet-A",
            None,
            &mut policies,
            now + std::time::Duration::from_secs(6),
            adb.to_str().unwrap(),
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(adb.with_extension("log")).unwrap(),
            "tablet-A\ntablet-A\n"
        );
        deliver_extra_token("tablet-B", None, &mut policies, now, adb.to_str().unwrap()).await;
        assert_eq!(
            std::fs::read_to_string(adb.with_extension("log")).unwrap(),
            "tablet-A\ntablet-A\ntablet-B\n"
        );
    }

    #[tokio::test]
    async fn t144_primary_and_extra_wait_for_both_reverse_mappings_and_retry() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let adb = root.path().join("adb");
        std::fs::write(
            &adb,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$0.log"
if [ "$3" = reverse ]; then
    case "$2:$4" in PRIMARY:tcp:8890|EXTRA:tcp:8891)
        if [ ! -e "$0.$2.failed" ]; then touch "$0.$2.failed"; exit 1; fi;;
    esac
else
    cat >/dev/null
fi
"#,
        )
        .unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        let now = std::time::Instant::now();
        for (serial, port) in [("PRIMARY", 19000), ("EXTRA", 19002)] {
            let request = TabletConnection {
                serial,
                video_port: port,
                input_port: port + 1,
                auto_launch: true,
                token: None,
                adb: adb.to_str().unwrap(),
            };
            let mut retry = RelaunchBackoff::default();
            let mut ready = request.prepare(&mut retry, now).await;
            assert!(!ready, "T144 {serial} became ready after a failed reverse");
            let log = adb.with_extension("log");
            let failed = std::fs::read_to_string(&log).unwrap();
            assert!(!failed.contains(&format!("-s {serial} shell")));
            if !ready {
                ready = request
                    .prepare(&mut retry, now + std::time::Duration::from_secs(1))
                    .await;
            }
            assert!(!ready);
            assert_eq!(
                std::fs::read_to_string(&log).unwrap(),
                failed,
                "retry must back off"
            );
            if !ready {
                ready = request
                    .prepare(&mut retry, now + std::time::Duration::from_secs(5))
                    .await;
            }
            assert!(ready, "unchanged serial never recovered");
            let passed = std::fs::read_to_string(&log).unwrap();
            assert!(passed.contains(&format!("-s {serial} reverse tcp:8890 tcp:{port}")));
            assert!(passed.contains(&format!("-s {serial} reverse tcp:8891 tcp:{}", port + 1)));
            assert_eq!(passed.matches(&format!("-s {serial} shell")).count(), 1);
        }
    }

    #[tokio::test]
    async fn t143_crashed_extra_apps_recover_with_per_device_backoff() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let adb = root.path().join("adb");
        std::fs::write(
            &adb,
            r#"#!/bin/sh
if [ "$4" = pidof ]; then
    if [ "$2" = LIVE ] || [ -e "$0.$2.alive" ]; then echo 123; exit 0; fi
    exit 1
fi
cat >/dev/null
printf '%s\n' "$2" >> "$0.log"
[ "$2" != FAILED ]
"#,
        )
        .unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        let assigned = ["LIVE", "DEAD", "FAILED"].map(String::from);
        let mut policies = std::collections::HashMap::new();
        let now = std::time::Instant::now();
        recover_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now,
            adb.to_str().unwrap(),
        )
        .await;
        let log = adb.with_extension("log");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap_or_default(),
            "DEAD\nFAILED\n"
        );
        recover_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(1),
            adb.to_str().unwrap(),
        )
        .await;
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "DEAD\nFAILED\n");
        recover_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(5),
            adb.to_str().unwrap(),
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "DEAD\nFAILED\nDEAD\nFAILED\n"
        );
        std::fs::write(adb.with_extension("DEAD.alive"), "").unwrap();
        recover_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(6),
            adb.to_str().unwrap(),
        )
        .await;
        std::fs::remove_file(adb.with_extension("DEAD.alive")).unwrap();
        recover_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(7),
            adb.to_str().unwrap(),
        )
        .await;
        let expected = "DEAD\nFAILED\nDEAD\nFAILED\nDEAD\n";
        assert_eq!(std::fs::read_to_string(&log).unwrap(), expected);
        recover_assigned_apps(
            &assigned,
            false,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(1000),
            adb.to_str().unwrap(),
        )
        .await;
        assert_eq!(std::fs::read_to_string(&log).unwrap(), expected);
    }

    #[tokio::test]
    async fn t142_every_slot_and_wifi_setup_require_the_app() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let adb = root.path().join("adb");
        std::fs::write(&adb, "#!/bin/sh\ncase \"$2\" in TABLET*|192.0.2.1:5555) echo package:/data/app/com.uscreen/base.apk;; esac\n").unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        let adb = adb.to_str().unwrap();
        let network = ["PHONE", "192.0.2.1:5555"].map(String::from);
        assert_eq!(
            pick_device_with(&network, None, adb).await.as_deref(),
            Some("192.0.2.1:5555")
        );
        assert_eq!(
            pick_device_with(&network, Some("PHONE"), adb)
                .await
                .as_deref(),
            Some("192.0.2.1:5555")
        );
        assert_eq!(wifi_device_with(&network, adb).await, None);
        let devices = ["PHONE", "TABLET_A", "TABLET_B"].map(String::from);
        let eligible = app_devices_with(&devices, adb).await;
        let primary = pick_device_with(&devices, None, adb).await;
        assert_eq!(primary.as_deref(), Some("TABLET_A"));
        assert_eq!(extra_devices(&eligible, primary.as_deref()), ["TABLET_B"]);
        assert_eq!(
            wifi_device_with(&devices, adb).await.as_deref(),
            Some("TABLET_A")
        );
        assert_eq!(
            pick_device_with(&devices, Some("TABLET_B"), adb)
                .await
                .as_deref(),
            Some("TABLET_B")
        );
        assert_eq!(pick_device_with(&["PHONE".into()], None, adb).await, None);
    }

    #[tokio::test]
    async fn t105_live_wifi_off_and_address_changes_reach_adb() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let adb = dir.path().join("adb");
        std::fs::write(
            &adb,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.log\"\necho connected\n",
        )
        .unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.path().join("config.toml");
        let write =
            |address| std::fs::write(&path, format!("wifi_address = \"{address}\"\n")).unwrap();
        write("192.0.2.1:5555");
        let reconnect = WifiReconnect::new(path.clone(), adb.to_str().unwrap().into());
        assert_eq!(reconnect.connect().await.as_deref(), Some("192.0.2.1:5555"));
        write("");
        assert_eq!(reconnect.connect().await, None);
        write("192.0.2.2:5555");
        assert_eq!(reconnect.connect().await.as_deref(), Some("192.0.2.2:5555"));
        assert_eq!(
            std::fs::read_to_string(adb.with_extension("log")).unwrap(),
            "connect 192.0.2.1:5555\nconnect 192.0.2.2:5555\n"
        );
    }

    #[tokio::test]
    async fn t106_usb_migration_preserves_physical_tablet_identity() {
        let mut identities = std::collections::HashMap::from([
            ("PHONE".into(), "phone".into()),
            ("TABLET".into(), "tablet".into()),
            ("192.0.2.1:5555".into(), "tablet".into()),
        ]);
        let devices = ["PHONE", "TABLET", "192.0.2.1:5555"].map(String::from);
        let selected =
            unique_devices(&devices, Some("192.0.2.1:5555"), &mut identities, "/unused").await;
        assert_eq!(
            current_transport(&selected, Some("192.0.2.1:5555"), &identities).as_deref(),
            Some("TABLET")
        );
        let without_cable = ["PHONE", "192.0.2.1:5555"].map(String::from);
        let selected =
            unique_devices(&without_cable, Some("TABLET"), &mut identities, "/unused").await;
        assert_eq!(
            current_transport(&selected, Some("TABLET"), &identities).as_deref(),
            Some("192.0.2.1:5555")
        );
        assert_eq!(
            current_transport(&selected, Some("192.0.2.1:5555"), &identities).as_deref(),
            Some("192.0.2.1:5555")
        );
    }

    #[tokio::test]
    async fn t107_dual_transports_use_one_physical_slot() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let adb = root.path().join("adb");
        std::fs::write(&adb, "#!/bin/sh\ncase \"$2\" in USB_A|192.0.2.1:5555) echo device-A;; USB_B) echo device-B;; esac\n").unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        let devices = ["USB_A", "USB_B", "192.0.2.1:5555"].map(String::from);
        let mut identities = std::collections::HashMap::new();
        assert_eq!(
            unique_devices(&devices, None, &mut identities, adb.to_str().unwrap()).await,
            ["USB_A", "USB_B"]
        );
        let selected = unique_devices(
            &devices,
            Some("192.0.2.1:5555"),
            &mut identities,
            adb.to_str().unwrap(),
        )
        .await;
        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&"USB_B".into()));
        assert!(selected.contains(&"USB_A".into()) ^ selected.contains(&"192.0.2.1:5555".into()));
        let unknown = ["UNKNOWN_A", "UNKNOWN_B"].map(String::from);
        assert_eq!(
            unique_devices(&unknown, None, &mut identities, adb.to_str().unwrap()).await,
            unknown
        );
    }

    #[test]
    fn t039_host_sends_tokens_only_to_the_protected_activity() {
        let token = "a".repeat(64);
        let cmd = super::app_launch_command(Some(&token));
        assert!(cmd.contains("com.uscreen/.TokenActivity --es token"));
        assert!(!cmd.contains(".MainActivity --es token"));
        assert!(super::app_launch_command(None).contains(".MainActivity"));
    }

    #[test]
    fn t065_dead_flags_are_rejected_instead_of_silently_ignored() {
        use clap::Parser;
        for args in [
            vec!["uscreen", "start", "--daemon"],
            vec!["uscreen", "start", "-d"],
            vec!["uscreen", "--display", "DVI-I-1"],
            vec!["uscreen", "--auto-vdisplay"],
        ] {
            assert!(
                super::Cli::try_parse_from(args.clone()).is_err(),
                "accepted {args:?}"
            );
        }
    }

    use super::*;

    fn t237_process_fixture(root: &std::path::Path) -> std::path::PathBuf {
        let source = root.join("child.c");
        let executable = root.join("uscreen");
        std::fs::write(&source, "#include <stdio.h>\n#include <unistd.h>\nint main(void) { puts(\"ready\"); fflush(stdout); for (;;) pause(); }\n").unwrap();
        assert!(std::process::Command::new("cc")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap()
            .success());
        executable
    }

    async fn t237_child(executable: &std::path::Path, args: &[&str]) -> tokio::process::Child {
        use tokio::io::AsyncBufReadExt;
        let mut child = tokio::process::Command::new(executable)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut ready = String::new();
        tokio::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .await
            .unwrap();
        assert_eq!(ready, "ready\n");
        child
    }

    #[tokio::test]
    async fn t237_discovery_recognizes_default_and_explicit_daemons() {
        let root = tempfile::tempdir().unwrap();
        let executable = t237_process_fixture(root.path());
        for (args, expected) in [
            (vec![], true),
            (vec!["start"], true),
            (vec!["--fps", "30"], true),
            (vec!["status"], false),
            (vec!["doctor"], false),
            (vec!["--helper", "start", "status"], false),
        ] {
            let mut child = t237_child(&executable, &args).await;
            let detected = other_daemons().contains(&child.id().unwrap());
            child.kill().await.unwrap();
            assert_eq!(detected, expected, "T237: command {args:?}");
        }
    }

    #[tokio::test]
    async fn t237_stop_recovers_missing_and_stale_pid_files_without_touching_other_commands() {
        let root = tempfile::tempdir().unwrap();
        let executable = t237_process_fixture(root.path());
        let pid_path = root.path().join("daemon.pid");
        let mut unrelated = t237_child(&executable, &["doctor"]).await;
        for stale in [false, true] {
            let mut daemon = t237_child(&executable, &[]).await;
            let pid = daemon.id().unwrap();
            assert!(!is_daemon_process(
                pid,
                unsafe { libc::getuid() }.wrapping_add(1)
            ));
            if stale {
                std::fs::write(&pid_path, unrelated.id().unwrap().to_string()).unwrap();
            }
            // Discovery is read-only; constrain all signals to fixture children.
            let recovered = other_daemons()
                .into_iter()
                .filter(|&found| found == pid)
                .collect();
            stop_daemon_at(&pid_path, recovered).await.unwrap();
            assert!(daemon.wait().await.unwrap().code().is_none());
            assert!(unrelated.try_wait().unwrap().is_none());
            assert!(!pid_path.exists());
        }
        unrelated.kill().await.unwrap();
    }

    #[tokio::test]
    async fn t244_status_recovers_daemon_despite_stale_pid_file() {
        if std::env::var_os("USCREEN_T244_CHILD").is_some() {
            show_status().await.unwrap();
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let executable = t237_process_fixture(root.path());
        let mut unrelated = t237_child(&executable, &["doctor"]).await;
        let pid_path = root.path().join(".local/share/uscreen/uscreen.pid");
        std::fs::create_dir_all(pid_path.parent().unwrap()).unwrap();
        let mut daemon = t237_child(&executable, &[]).await;
        let pid = daemon.id().unwrap();
        let mut reports = Vec::new();
        for stale in [
            "4294967295".to_string(),
            "0".into(),
            "broken".into(),
            unrelated.id().unwrap().to_string(),
        ] {
            std::fs::write(&pid_path, &stale).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "cli_tests::t244_status_recovers_daemon_despite_stale_pid_file",
                    "--nocapture",
                ])
                .env("USCREEN_T244_CHILD", "1")
                .env("HOME", root.path())
                .output()
                .unwrap();
            reports.push((stale, output));
        }
        daemon.kill().await.unwrap();
        unrelated.kill().await.unwrap();
        for (stale, output) in reports {
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            let reported = text
                .split("uscreen is running (PID: ")
                .nth(1)
                .and_then(|tail| tail.split(')').next())
                .unwrap_or("");
            assert!(
                reported
                    .split_whitespace()
                    .any(|value| value == pid.to_string()),
                "T244: PID file {stale:?} hid daemon {pid}: {text}"
            );
            assert!(
                !reported.split_whitespace().any(|value| value == stale),
                "T244: unrelated process reported: {text}"
            );
        }
    }

    #[tokio::test]
    async fn t030_pid_reuse_does_not_mistake_another_process_for_uscreen() {
        let mut child = tokio::process::Command::new("sleep")
            .arg("5")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let reported = config::daemon_is_running(child.id().unwrap());
        child.kill().await.unwrap();
        assert!(!reported);
    }

    #[tokio::test]
    async fn t033_stop_waits_for_daemon_cleanup() {
        use tokio::io::AsyncBufReadExt;
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "echo uscreen > /proc/$$/comm; trap 'sleep 0.1; exit 0' TERM; echo ready; while :; do sleep 0.01; done"])
            .stdout(std::process::Stdio::piped()).kill_on_drop(true).spawn().unwrap();
        let mut ready = String::new();
        tokio::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .await
            .unwrap();
        assert_eq!(ready, "ready\n");
        stop_pids(&[child.id().unwrap()]).await.unwrap();
        let finished = child.try_wait().unwrap().is_some();
        if !finished {
            child.wait().await.unwrap();
        }
        assert!(
            finished,
            "stop returned before the daemon completed cleanup"
        );
    }

    #[test]
    fn t032_extra_tablets_exclude_the_selected_primary_not_the_first_entry() {
        let devices = vec!["new-usb".into(), "current-usb".into()];
        assert_eq!(extra_devices(&devices, Some("current-usb")), ["new-usb"]);
        assert_eq!(extra_devices(&devices, None), devices);
    }

    #[test]
    fn t067_manual_forwarding_uses_custom_host_ports() {
        let hints = forwarding_instructions(9000, 9001);
        assert!(hints.contains("adb reverse tcp:8890 tcp:9000"));
        assert!(hints.contains("adb reverse tcp:8891 tcp:9001"));
    }

    #[test]
    fn t069_each_disappearance_allows_a_new_wifi_announcement() {
        let mut current = Some("tablet".into());
        let mut announced = true;
        disconnected_primary(&mut current, &mut announced);
        assert_eq!(current, None);
        assert!(!announced);
    }

    #[tokio::test]
    async fn t031_server_bind_failure_prevents_successful_startup() {
        for occupied_input in [false, true] {
            let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = occupied.local_addr().unwrap().port();
            let stream = stream::StreamServer::new(
                stream::StreamConfig {
                    video_port: if occupied_input { 0 } else { port },
                    token: None,
                },
                Default::default(),
                Default::default(),
            );
            let (settings_tx, _settings_rx) = watch::channel(capture::EncoderSettings {
                encoder: "libx264".into(),
                fps: 60,
                bitrate: 20000,
                width: 1920,
                height: 1080,
                quality: 18,
                width_mm: 310,
                height_mm: 194,
                stream_scale: 1,
                geometry_ready: true,
            });
            let (mode_tx, _mode_rx) = watch::channel(false);
            let (_card_tx, card_rx) = watch::channel(None);
            let (_tablet_tx, tablet_rx) = watch::channel(false);
            let input = input::InputServer::new(
                input::InputConfig {
                    port: if occupied_input { port } else { 0 },
                    touch: false,
                    pen: false,
                    pointer: false,
                    ..Default::default()
                },
                Some(settings_tx),
                mode_tx,
                latency::LatencyTracker::new(),
                Default::default(),
                card_rx,
                tablet_rx,
            );
            let (video_tx, _) = broadcast::channel(8);
            let result = start_servers(stream, input, video_tx).await;
            let failed = result.is_err();
            if let Ok((stream, input)) = result {
                stream.abort();
                input.abort();
            }
            assert!(
                failed,
                "occupied_input={occupied_input}: startup must report bind failure"
            );
        }
    }

    #[tokio::test]
    async fn t068_extra_session_waits_for_capture_cleanup() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let completed = Arc::new(AtomicBool::new(false));
        let done = completed.clone();
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let capture = tokio::spawn(async move {
            stop_rx.changed().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
            done.store(true, Ordering::SeqCst);
        });
        let (tablet_tx, _tablet_rx) = watch::channel(true);
        ExtraSession {
            instance: 1,
            tablet_tx,
            relaunch: Default::default(),
            stop_tx,
            tasks: vec![],
            capture,
            video_port: 0,
            input_port: 0,
        }
        .stop()
        .await;
        assert!(
            completed.load(Ordering::SeqCst),
            "capture must finish before stop returns"
        );
    }

    #[tokio::test]
    async fn t072_launched_children_are_reaped_after_exit() {
        let pid = config::spawn_reaped(&mut std::process::Command::new("true")).unwrap();
        let path = format!("/proc/{pid}");
        let reaped = tokio::time::timeout(std::time::Duration::from_millis(300), async {
            while std::path::Path::new(&path).exists() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        // Reap the old implementation's zombie even when the assertion fails.
        if !reaped {
            unsafe {
                libc::waitpid(pid as i32, std::ptr::null_mut(), 0);
            }
        }
        assert!(reaped, "launcher left a zombie process");
    }

    #[test]
    fn t066_cli_overrides_use_the_same_limits_as_saved_settings() {
        let cli = Cli::try_parse_from([
            "uscreen",
            "--fps",
            "500",
            "--bitrate",
            "0",
            "--width",
            "8192",
            "--height",
            "1",
            "--quality",
            "99",
            "--stream-scale",
            "0",
        ])
        .unwrap();
        let effective = effective_config(&cli, &config::FileConfig::default());
        assert_eq!(effective.fps, config::MAX_FPS);
        assert_eq!(effective.bitrate, config::MIN_BITRATE_KBPS);
        assert_eq!((effective.width, effective.height), (4095, 480));
        assert_eq!(effective.quality, config::MAX_QUALITY);
        assert_eq!(effective.stream_scale, 1);
    }
}

async fn start_servers(
    stream_srv: stream::StreamServer,
    input_srv: input::InputServer,
    video_tx: broadcast::Sender<capture::VideoPacket>,
) -> Result<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)> {
    // Bind both sockets before starting any worker. A failed second bind
    // drops the first listener and reports startup failure to the caller.
    let video_listener = stream_srv.bind().await?;
    let input_listener = input_srv.bind().await?;
    let stream_handle = tokio::spawn(async move {
        if let Err(e) = stream_srv.run_with_listener(video_tx, video_listener).await {
            error!("Stream server failed: {}", e);
        }
    });
    let input_handle = tokio::spawn(async move {
        if let Err(e) = input_srv.run_with_listener(input_listener).await {
            error!("Input server failed: {}", e);
        }
    });
    Ok((stream_handle, input_handle))
}

async fn run_daemon(cli: Cli) -> Result<()> {
    let helper_path = find_helper(cli.helper.as_deref())?;
    let pid_path = get_pid_path();
    if let Some(parent) = pid_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    ensure_single_daemon(&pid_path)?;

    // Validate the complete effective range before claiming resources.
    let file_cfg = config::FileConfig::load();
    let effective = effective_config(&cli, &file_cfg);
    config::slot_ports(
        effective.video_port,
        effective.input_port,
        effective.max_tablets,
    )?;

    // Write PID file for clean stop/status
    let pid = std::process::id();
    std::fs::write(&pid_path, pid.to_string())?;

    heal_config(&file_cfg);
    let encoder = effective.encoder.clone();
    let fps = effective.fps;
    let bitrate = effective.bitrate;
    let width = effective.width;
    let height = effective.height;
    let video_port = effective.video_port;
    let input_port = effective.input_port;
    let quality = effective.quality;
    let stream_scale = effective.stream_scale;
    let pen_only = initial_pen_only(cli.pen_only, &file_cfg);

    let cap_config = capture::CaptureConfig {
        helper_path: helper_path.clone(),
        edid_path: cli.edid.clone(),
        encoder: encoder.clone(),
        vaapi_device: file_cfg.vaapi_device.clone(),
        fps,
        bitrate,
        width,
        height,
        quality,
        // Replaced as soon as the tablet reports its real panel size.
        width_mm: edid::DEFAULT_WIDTH_MM,
        height_mm: edid::DEFAULT_HEIGHT_MM,
        stream_scale,
        position: config::Position::parse_or_default(&file_cfg.position),
        ten_bit: file_cfg.ten_bit,
        instance: 0,
        // With several tablets every helper is pinned to its own card; the
        // helper's own search would give each of them the same one.
        card: if file_cfg.max_tablets > 1 {
            vdisplay::evdi_cards().into_iter().min()
        } else {
            None
        },
    };

    let token = create_session_token(file_cfg.require_token)?;
    let relaunch = std::sync::Arc::new(tokio::sync::Notify::new());

    let stream_config = stream::StreamConfig {
        video_port,
        token: token.clone(),
    };

    let input_config = input::InputConfig {
        port: input_port,
        instance: 0,
        token: token.clone(),
        codec: capture::Codec::from_encoder(&encoder).muxer().to_string(),
        virtual_width: width,
        virtual_height: height,
        touch: file_cfg.input_touch,
        pen: file_cfg.input_pen,
        pointer: file_cfg.input_pointer,
    };

    // Which of the two jobs the tablet is doing. Switchable at runtime from
    // the tablet's own settings, so it lives in a channel the input server,
    // the capture manager and the config writer all follow.
    let (mode_tx, _) = watch::channel(pen_only);

    // Tablet control messages update these settings and restart the encoder.
    // config.toml is read at daemon startup; file edits require a daemon restart
    // (the GUI's Apply & Restart does this). Wi-Fi reconnect reads its address
    // from disk separately on each attempt.
    let (settings_tx, settings_rx) = watch::channel(capture::EncoderSettings {
        encoder: encoder.clone(),
        fps,
        bitrate,
        width,
        height,
        quality,
        width_mm: edid::DEFAULT_WIDTH_MM,
        height_mm: edid::DEFAULT_HEIGHT_MM,
        stream_scale,
        geometry_ready: false,
    });

    // Tablet presence, published by the ADB monitor.
    let (tablet_tx, tablet_rx) = watch::channel(false);
    let tray_tablet_rx = tablet_tx.subscribe();

    let mut capture_mgr = capture::CaptureManager::new(cap_config);
    let card_rx = capture_mgr.card_rx();
    let codec_config = capture_mgr.codec_config_arc();
    let latency = capture_mgr.latency_tracker();
    let stream_srv =
        stream::StreamServer::new(stream_config, codec_config, capture_mgr.idr_request_flag());
    let input_srv = input::InputServer::new(
        input_config,
        Some(settings_tx.clone()),
        mode_tx.clone(),
        latency,
        relaunch.clone(),
        card_rx,
        tablet_tx.subscribe(),
    );

    // Deliberately shallow. This ring is pure latency when it fills: 256 frames
    // is four seconds of backlog at 60 fps, and a client that fell behind would
    // dutifully receive all of it instead of skipping to something current.
    // At 8, `RecvError::Lagged` fires early and the skip-to-newest-IDR path in
    // the stream server actually gets a chance to run.
    let (video_tx, _) = broadcast::channel(8);

    info!("=== uscreen daemon starting ===");
    info!("  Resolution: {}x{} @ {}fps", width, height, fps);
    info!("  Encoder: {}", encoder);
    info!("  Bitrate: {} kbps", bitrate);
    info!("  Stream port: {}", video_port);
    info!("  Input port: {}", input_port);
    info!(
        "  Input devices: touch={} pen={} pointer={}",
        file_cfg.input_touch, file_cfg.input_pen, file_cfg.input_pointer
    );

    // What the capture manager actually follows: the virtual display should
    // exist exactly while a tablet is attached *and* being used as a screen.
    // Folding the mode in here is what makes switching live — the capture
    // manager already knows how to raise and tear down the display on this
    // signal, and cannot tell the difference between an unplugged tablet and
    // one that is currently a drawing surface.
    let (gate_tx, gate_rx) = watch::channel(false);
    spawn_display_gate(gate_tx, tablet_rx, mode_tx.subscribe(), settings_tx.clone());

    // Cooperative shutdown: the capture task must get a chance to kill and reap
    // ffmpeg/evdi_helper before the process exits, or they linger holding the
    // capture FIFO and collide with the next start.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    if pen_only {
        info!("Starting in pen-only mode: the tablet drives this machine's own");
        info!("  screen with the pen. No virtual display, no encoding.");
    }

    let (stream_handle, input_handle) =
        start_servers(stream_srv, input_srv, video_tx.clone()).await?;

    let video_tx_cap = video_tx.clone();
    let settings_rx_cap = settings_rx.clone();
    let mut cap_handle = tokio::spawn(async move {
        // Started in both modes. In pen-only the gate below holds the virtual
        // output disabled, with no helper or encoder until a tablet requests
        // second-screen mode.
        if let Err(e) = capture_mgr
            .stream_frames(video_tx_cap, settings_rx_cap, gate_rx, shutdown_rx)
            .await
        {
            error!("Capture manager failed: {}", e);
        }
    });

    // Fields the user overrode on the command line for this run only. They
    // must not be written back: a flag is not a settings change, and
    // persisting one silently rewrites the user's configuration behind them.
    let cli_overrides = CliOverrides::new(&cli);
    let save_handle = tokio::spawn(persist_settings(settings_rx.clone(), cli_overrides));

    // Remember which mode the tablet was left in. Unlike the --pen-only flag,
    // which is a one-off for this run and never written back, a switch made
    // from the tablet is a deliberate choice and should survive a restart.
    let mode_save_handle = tokio::spawn(persist_mode(mode_tx.subscribe()));

    // The daemon's only face on the desktop. It follows the same channels the
    // rest of the daemon does, so it cannot drift out of step with what is
    // actually running.
    // Once a day, ask GitHub whether there is a newer release. Reported in
    // the tray and by doctor; never installed from here.
    let (update_tx, update_rx) = watch::channel::<update::Available>(None);
    let update_handle = if file_cfg.check_updates {
        Some(tokio::spawn(async move { update::run(update_tx).await }))
    } else {
        None
    };

    let tray_mode_tx = mode_tx.clone();
    let tray_shutdown_tx = shutdown_tx.clone();
    let tray_pen_device = file_cfg.input_pen;
    let tray_handle = tokio::spawn(async move {
        tray::run(
            tray_mode_tx,
            tray_tablet_rx,
            tray_shutdown_tx,
            update_rx,
            tray_pen_device,
        )
        .await;
    });

    // Plug-and-play: watch for the tablet over ADB, set up port forwarding
    // and launch the app whenever it's (re)connected.
    let auto_launch = file_cfg.auto_launch_app;
    let adb_token = token.clone();
    let extra = ExtraSessionTemplate {
        max_tablets: file_cfg.max_tablets,
        cap_template: capture::CaptureConfig {
            helper_path: helper_path.clone(),
            edid_path: None,
            encoder: encoder.clone(),
            vaapi_device: file_cfg.vaapi_device.clone(),
            fps,
            bitrate,
            width,
            height,
            quality,
            width_mm: edid::DEFAULT_WIDTH_MM,
            height_mm: edid::DEFAULT_HEIGHT_MM,
            stream_scale,
            position: config::Position::parse_or_default(&file_cfg.position),
            ten_bit: file_cfg.ten_bit,
            instance: 0,
            card: None,
        },
        video_port,
        input_port,
        codec: capture::Codec::from_encoder(&encoder).muxer().to_string(),
        input_touch: file_cfg.input_touch,
        input_pen: file_cfg.input_pen,
        input_pointer: file_cfg.input_pointer,
        token: token.clone(),
        mode_tx: mode_tx.clone(),
        shutdown_rx: shutdown_tx.subscribe(),
    };
    let mut adb_handle = tokio::spawn(async move {
        adb_monitor(
            video_port,
            input_port,
            auto_launch,
            tablet_tx,
            adb_token,
            relaunch,
            extra,
        )
        .await;
    });

    println!();
    println!("================================================");
    println!("  uscreen daemon running (PID: {})", pid);
    println!("================================================");
    println!("  On your tablet, open the UScreen app");
    println!("  ADB ports will be auto-forwarded if possible.");
    println!("  Otherwise, run:");
    println!("{}", forwarding_instructions(video_port, input_port));
    println!("================================================");
    println!();

    // `uscreen stop` (and the GUI) send SIGTERM, not SIGINT — without a
    // handler for it, the kernel kills the process with its default
    // disposition and none of our cleanup (which is what kills the
    // evdi_helper/ffmpeg children via kill_on_drop) ever runs, orphaning
    // them to fight over the shared FIFO with the next daemon that starts.
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    // Quit from the tray raises the same flag the signal handlers do, so it
    // has to be waited on here too — otherwise the pipeline winds down while
    // the process itself stays alive with nothing left to run.
    let mut quit_rx = shutdown_tx.subscribe();
    tokio::select! {
        _ = signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
        _ = async { while quit_rx.changed().await.is_ok() && !*quit_rx.borrow() {} } => {}
    }
    info!("Shutting down...");

    // Ask the capture pipeline to wind down, and give it a bounded moment to
    // actually reap its children before pulling the rug out.
    let _ = shutdown_tx.send(true);
    if tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let _ = tokio::join!(&mut cap_handle, &mut adb_handle);
    })
    .await
    .is_err()
    {
        warn!("Capture pipelines did not stop within 5s");
        cap_handle.abort();
        adb_handle.abort();
    }

    // The input server restores the keyboard when the last tablet detaches;
    // this covers a shutdown with a tablet still attached.
    osk::restore().await;

    stream_handle.abort();
    input_handle.abort();
    adb_handle.abort();
    save_handle.abort();
    mode_save_handle.abort();
    tray_handle.abort();
    if let Some(h) = update_handle {
        h.abort();
    }

    // Clean up PID file
    // Only if it is still ours. Winding down takes a few seconds, and a
    // daemon started in the meantime has already written its own PID here;
    // deleting that leaves the new daemon untracked - doctor calls it an
    // orphan and `uscreen stop` can no longer find it.
    remove_pid_file_if_ours(&pid_path, pid);

    info!("uscreen daemon stopped");
    Ok(())
}

fn ensure_single_daemon(pid_path: &std::path::Path) -> Result<()> {
    // Refuse to start a second daemon on top of a live one: the PID file is
    // a single slot, so `uscreen stop` only ever kills the most recently
    // started process — any earlier instance still running would become
    // permanently untracked, and both would keep writing/reading the same
    // EVDI FIFO, corrupting frames and starving the encoder.
    if let Ok(existing) = std::fs::read_to_string(pid_path) {
        if let Ok(existing_pid) = existing.trim().parse::<i32>() {
            let alive = is_daemon_process(existing_pid as u32, unsafe { libc::getuid() });
            if alive {
                anyhow::bail!(
                    "uscreen daemon already running (PID: {}). Run `uscreen stop` first.",
                    existing_pid
                );
            }
        }
    }

    // A daemon that lost its PID file (see remove_pid_file_if_ours) is still
    // a daemon; two of them fight over the EVDI device and the FIFO.
    let others = other_daemons();
    if !others.is_empty() {
        anyhow::bail!(
            "uscreen daemon already running (PID {:?}, untracked). Run `uscreen stop` first.",
            others
        );
    }

    Ok(())
}

fn heal_config(file_cfg: &config::FileConfig) {
    // `load()` clamps unusable values, but leaving the bad number on disk means
    // the GUI keeps showing it and writes it straight back. Heal the file once,
    // here, so every tool agrees on what the settings actually are.
    let raw: Option<config::FileConfig> = std::fs::read_to_string(config::config_path())
        .ok()
        .and_then(|t| toml::from_str(&t).ok());
    if raw.is_some_and(|r| &r != file_cfg) {
        match config::FileConfig::update(|_| Ok(())) {
            Ok(_) => info!(
                "Rewrote out-of-range settings in {:?}",
                config::config_path()
            ),
            Err(e) => warn!("Could not rewrite the config file: {}", e),
        }
    }
}

fn initial_pen_only(requested: bool, file_cfg: &config::FileConfig) -> bool {
    let mut pen_only = requested || file_cfg.pen_only;
    if pen_only && !file_cfg.input_pen {
        warn!("Pen-only mode needs the pen device, but input_pen is off in config.toml — starting as a second screen");
        pen_only = false;
    }

    pen_only
}

fn create_session_token(required: bool) -> Result<Option<String>> {
    // One secret per daemon run. Handed to the app over adb when it is
    // launched; anything connecting to the loopback ports without it gets
    // nothing. See config::FileConfig::require_token.
    let token: Option<String> = if required {
        // Fail closed. Running without a token because the runtime dir was
        // unwritable would quietly turn a required check into no check.
        match runtime::new_session_token() {
            Ok(t) => Some(t),
            Err(e) => anyhow::bail!(
                "require_token is on but no session token could be created: {}. \
                 Fix the runtime directory, or set require_token = false.",
                e
            ),
        }
    } else {
        warn!("require_token = false: any local process can read the screen and inject input");
        None
    };
    Ok(token)
}

fn spawn_display_gate(
    gate_tx: watch::Sender<bool>,
    mut tablet_rx: watch::Receiver<bool>,
    mut mode_rx: watch::Receiver<bool>,
    settings: watch::Sender<capture::EncoderSettings>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last = false;
        loop {
            let attached = *tablet_rx.borrow();
            if !attached {
                settings.send_if_modified(|s| {
                    let was_ready = s.geometry_ready;
                    s.geometry_ready = false;
                    was_ready
                });
            }
            let active = attached && !*mode_rx.borrow();
            if active != last {
                last = active;
                let _ = gate_tx.send(active);
            }
            tokio::select! {
                r = tablet_rx.changed() => if r.is_err() { break },
                r = mode_rx.changed() => if r.is_err() { break },
            }
        }
    })
}

async fn persist_settings(
    mut settings_rx: watch::Receiver<capture::EncoderSettings>,
    cli_overrides: CliOverrides,
) {
    let mut previous = settings_rx.borrow().clone();
    while settings_rx.changed().await.is_ok() {
        let s = settings_rx.borrow().clone();
        let result = config::FileConfig::update(|cfg| {
            cli_overrides.apply_encoder(cfg, &s, &previous);
            cli_overrides.apply_geometry(cfg, &s, &previous);
            Ok(())
        });
        previous = s;
        if let Err(e) = result {
            warn!("Failed to persist settings: {}", e);
        } else {
            info!("Settings saved to {:?}", config::config_path());
        }
    }
}

struct CliOverrides {
    encoder: bool,
    fps: bool,
    bitrate: bool,
    width: bool,
    height: bool,
    quality: bool,
    stream_scale: bool,
}

impl CliOverrides {
    fn new(cli: &Cli) -> Self {
        Self {
            encoder: cli.encoder.is_some(),
            fps: cli.fps.is_some(),
            bitrate: cli.bitrate.is_some(),
            width: cli.width.is_some(),
            height: cli.height.is_some(),
            quality: cli.quality.is_some(),
            stream_scale: cli.stream_scale.is_some(),
        }
    }
    fn apply_encoder(
        &self,
        cfg: &mut config::FileConfig,
        s: &capture::EncoderSettings,
        previous: &capture::EncoderSettings,
    ) {
        if !self.encoder && s.encoder != previous.encoder {
            cfg.encoder = s.encoder.clone();
        }
        if !self.fps && s.fps != previous.fps {
            cfg.fps = s.fps;
        }
        if !self.bitrate && s.bitrate != previous.bitrate {
            cfg.bitrate = s.bitrate;
        }
        if !self.quality && s.quality != previous.quality {
            cfg.quality = s.quality;
        }
    }
    fn apply_geometry(
        &self,
        cfg: &mut config::FileConfig,
        s: &capture::EncoderSettings,
        previous: &capture::EncoderSettings,
    ) {
        if !self.width && s.width != previous.width {
            cfg.width = s.width;
        }
        if !self.height && s.height != previous.height {
            cfg.height = s.height;
        }
        if !self.stream_scale && s.stream_scale != previous.stream_scale {
            cfg.stream_scale = s.stream_scale;
        }
    }
}

async fn persist_mode(mut mode_rx: watch::Receiver<bool>) {
    while mode_rx.changed().await.is_ok() {
        let pen_only = *mode_rx.borrow();
        if let Err(e) = config::FileConfig::update(|cfg| {
            cfg.pen_only = pen_only;
            Ok(())
        }) {
            warn!("Failed to persist mode: {}", e);
        }
    }
}

/// Recover same-user daemons even when their PID file is missing.
fn other_daemons() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let uid = unsafe { libc::getuid() };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|&pid| pid != std::process::id() && is_daemon_process(pid, uid))
        .collect()
}

fn is_daemon_process(pid: u32, uid: u32) -> bool {
    use std::os::unix::{ffi::OsStringExt, fs::MetadataExt};
    let base = PathBuf::from(format!("/proc/{pid}"));
    if std::fs::metadata(&base).map(|m| m.uid()).ok() != Some(uid)
        || !config::daemon_is_running(pid)
    {
        return false;
    }
    let Ok(cmdline) = std::fs::read(base.join("cmdline")) else {
        return false;
    };
    if cmdline.is_empty() {
        return false;
    }
    let args = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| std::ffi::OsString::from_vec(arg.to_vec()));
    // Use the real parser: option values named "start" are not subcommands.
    Cli::try_parse_from(args).is_ok_and(|cli| matches!(cli.command, None | Some(Commands::Start)))
}

fn remove_pid_file_if_ours(pid_path: &std::path::Path, pid: u32) {
    let ours = std::fs::read_to_string(pid_path)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .map(|p| p == pid)
        .unwrap_or(false);
    if ours {
        let _ = std::fs::remove_file(pid_path);
    }
}

fn get_pid_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{}/.local/share/uscreen/uscreen.pid", home))
}

fn select_helper(explicit: Option<&std::path::Path>, candidates: &[PathBuf]) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let executable = |path: &std::path::Path| {
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    };
    if let Some(path) = explicit {
        anyhow::ensure!(
            executable(path),
            "Explicit --helper {} is not an executable file",
            path.display()
        );
        return path.canonicalize().context("resolve explicit --helper");
    }
    candidates
        .iter()
        .find(|path| executable(path))
        .map(|path| path.canonicalize())
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("No EVDI helper found; build/install it or set --helper"))
}

/// Explicit overrides are validated without fallback. Otherwise prefer the
/// helper belonging to this executable's installation, then generic installs
/// and development checkout locations.
fn find_helper(explicit: Option<&std::path::Path>) -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let exe = std::env::current_exe().ok();
    let mut candidates = Vec::new();
    if let Some(dir) = exe.as_ref().and_then(|path| path.parent()) {
        candidates.push(dir.join("evdi_helper"));
        if let Some(prefix) = dir.parent() {
            for relative in [
                "lib/uscreen/evdi_helper",
                "lib64/uscreen/evdi_helper",
                "libexec/uscreen/evdi_helper",
            ] {
                candidates.push(prefix.join(relative));
            }
        }
    }
    candidates.push(PathBuf::from(home).join(".local/bin/evdi_helper"));
    for path in [
        "/usr/lib/uscreen/evdi_helper",
        "/usr/lib64/uscreen/evdi_helper",
        "/usr/libexec/uscreen/evdi_helper",
        "/usr/local/lib/uscreen/evdi_helper",
        "host/evdi/evdi_helper",
    ] {
        candidates.push(PathBuf::from(path));
    }
    if let Some(exe) = exe {
        for dir in exe.ancestors().skip(1).take(5) {
            candidates.push(dir.join("host/evdi/evdi_helper"));
        }
    }
    select_helper(explicit, &candidates)
}

/// Everything needed to bring up a pipeline for a second (third, ...)
/// tablet on demand. The first tablet is wired at startup like it always
/// was; these are spawned when another serial shows up and torn down when
/// it goes.
struct ExtraSessionTemplate {
    max_tablets: u32,
    cap_template: capture::CaptureConfig,
    video_port: u16,
    input_port: u16,
    codec: String,
    token: Option<String>,
    /// Which virtual input devices each extra tablet gets; same switches as
    /// the first one.
    input_touch: bool,
    input_pen: bool,
    input_pointer: bool,
    mode_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

/// One extra tablet's running pipeline.
struct ExtraSession {
    instance: u32,
    tablet_tx: watch::Sender<bool>,
    relaunch: std::sync::Arc<tokio::sync::Notify>,
    stop_tx: watch::Sender<bool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    capture: tokio::task::JoinHandle<()>,
    video_port: u16,
    input_port: u16,
}

impl ExtraSession {
    async fn stop(mut self) {
        let _ = self.tablet_tx.send(false);
        let _ = self.stop_tx.send(true);
        // Let the capture manager disable its output and reap its children
        // before the tasks are dropped.
        if tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.capture)
            .await
            .is_err()
        {
            warn!("Capture pipeline {} did not stop within 5s", self.instance);
            self.capture.abort();
            let _ = self.capture.await;
        }
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

/// A multi-tablet slot must have an assigned card before any task starts.
fn assign_slot_card(
    config: &mut capture::CaptureConfig,
    cards: &[u32],
    instance: u32,
) -> Result<()> {
    config.card = Some(*cards.get(instance as usize).with_context(|| {
        format!(
            "tablet slot {} needs its own EVDI card; only {} available",
            instance + 1,
            cards.len()
        )
    })?);
    Ok(())
}

/// Bring up capture, stream and input for tablet number `instance`.
/// Ports are the base ports plus 2 per instance; the tablet side keeps
/// using 8890/8891, since `adb reverse` maps them per device.
async fn spawn_extra_session(t: &ExtraSessionTemplate, instance: u32) -> Result<ExtraSession> {
    let (video_port, input_port) = *config::slot_ports(t.video_port, t.input_port, t.max_tablets)?
        .get(instance as usize)
        .context("tablet slot outside configured range")?;
    let cards = vdisplay::evdi_cards();
    let mut cfg = t.cap_template.clone();
    cfg.instance = instance;
    assign_slot_card(&mut cfg, &cards, instance)?;

    let (settings_tx, settings_rx) = watch::channel(capture::EncoderSettings {
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
    });
    let (tablet_tx, tablet_rx) = watch::channel(false);
    let mut cap = capture::CaptureManager::new(cfg.clone());
    let card_rx = cap.card_rx();
    let codec_config = cap.codec_config_arc();
    let latency = cap.latency_tracker();
    let relaunch = std::sync::Arc::new(tokio::sync::Notify::new());
    let stream_srv = stream::StreamServer::new(
        stream::StreamConfig {
            video_port,
            token: t.token.clone(),
        },
        codec_config,
        cap.idr_request_flag(),
    );
    let input_srv = input::InputServer::new(
        input::InputConfig {
            port: input_port,
            instance,
            token: t.token.clone(),
            codec: t.codec.clone(),
            virtual_width: cfg.width,
            virtual_height: cfg.height,
            touch: t.input_touch,
            pen: t.input_pen,
            pointer: t.input_pointer,
        },
        Some(settings_tx.clone()),
        t.mode_tx.clone(),
        latency,
        relaunch.clone(),
        card_rx,
        tablet_tx.subscribe(),
    );

    let (gate_tx, gate_rx) = watch::channel(false);
    let (stop_tx, stop_rx) = watch::channel(false);
    let (video_tx, _) = broadcast::channel(8);
    let (stream_handle, input_handle) =
        start_servers(stream_srv, input_srv, video_tx.clone()).await?;
    let mut tasks = vec![stream_handle, input_handle];
    tasks.push(spawn_display_gate(
        gate_tx,
        tablet_rx,
        t.mode_tx.subscribe(),
        settings_tx,
    ));
    // Either the whole daemon stopping or this session being torn down
    // must wind the capture pipeline down cleanly.
    let mut daemon_stop = t.shutdown_rx.clone();
    let (cap_stop_tx, cap_stop_rx) = watch::channel(false);
    let mut stop_rx_c = stop_rx.clone();
    tasks.push(tokio::spawn(async move {
        tokio::select! {
            _ = daemon_stop.changed() => {}
            _ = stop_rx_c.changed() => {}
        }
        let _ = cap_stop_tx.send(true);
    }));
    let capture = tokio::spawn(async move {
        if let Err(e) = cap
            .stream_frames(video_tx, settings_rx, gate_rx, cap_stop_rx)
            .await
        {
            error!("Capture manager {} failed: {}", instance, e);
        }
    });
    info!(
        "Tablet slot {} ready: video port {}, input port {}{}",
        instance + 1,
        video_port,
        input_port,
        cfg.card
            .map(|c| format!(", EVDI card{}", c))
            .unwrap_or_default()
    );
    Ok(ExtraSession {
        instance,
        tablet_tx,
        relaunch,
        stop_tx,
        tasks,
        capture,
        video_port,
        input_port,
    })
}

/// Keeps watching for tablets. The first serial seen gets the pipeline wired
/// at startup; with max_tablets > 1, further serials each get a pipeline of
/// their own for as long as they stay attached.
///
/// Presence is published on `tablet_tx` so the capture manager can bring the
/// virtual display up and down along with the tablet.
#[allow(clippy::too_many_arguments)]
async fn adb_monitor(
    video_port: u16,
    input_port: u16,
    auto_launch: bool,
    tablet_tx: watch::Sender<bool>,
    token: Option<String>,
    relaunch: std::sync::Arc<tokio::sync::Notify>,
    extra: ExtraSessionTemplate,
) {
    let mut ledger = session_ledger();
    let mut daemon_stop = extra.shutdown_rx.clone();
    let mut state = TabletMonitor::new();
    // Polls since the app process was last checked. A tablet that is plugged
    // in but whose app has gone (swiped out of recents, killed by Android to
    // free memory, crashed) used to stay a blank screen until the cable was
    // pulled and put back; now the app comes back by itself. Checked every
    // fifth poll, so a missing app costs one `adb shell` every ten seconds.
    let mut polls_since_check: u32 = 0;
    const APP_CHECK_EVERY: u32 = 5;
    let reconnect = WifiReconnect::new(config::config_path(), "adb".into());
    if tokio::process::Command::new("adb")
        .arg("version")
        .output_bounded()
        .await
        .is_err()
    {
        error!("adb is not installed — the tablet can never be found. Install android-tools (or adb) and restart.");
    }

    loop {
        if *daemon_stop.borrow() {
            break;
        }
        let mut devices = adb_devices().await;
        add_fake_tablets(&mut devices);
        let devices = unique_devices(
            &devices,
            state.current.as_deref(),
            &mut state.identities,
            "adb",
        )
        .await;
        let devices = app_devices_with(&devices, "adb").await;
        let preferred = current_transport(&devices, state.current.as_deref(), &state.identities);
        let found = select_tablet(&devices, preferred.as_deref());

        state
            .forwarding_backoff
            .retain(|serial, _| devices.contains(serial));
        state
            .update_primary(
                &found,
                (video_port, input_port),
                auto_launch,
                token.as_deref(),
                &tablet_tx,
            )
            .await;

        state
            .sync_extra_sessions(
                &extra,
                &devices,
                found.as_deref(),
                auto_launch,
                token.as_deref(),
            )
            .await;

        state.extra_backoff.retain(|serial, _| {
            state.current.as_ref() == Some(serial) || state.extras.contains_key(serial)
        });

        state.publish_sessions(&mut ledger, video_port, input_port);

        // Poll every two seconds, but wake at once if a client turned up
        // without the token: the app was started by hand, and launching it
        // again over adb is how it gets one. Rate-limited so a misbehaving
        // client cannot make us hammer adb.
        let requests: Vec<_> = state
            .extras
            .iter()
            .map(|(serial, session)| (serial.clone(), session.relaunch.clone()))
            .collect();
        let extra_relaunch = wait_extra_relaunch(requests);
        tokio::select! {
            _ = daemon_stop.changed() => break,
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                polls_since_check += 1;
                if polls_since_check < APP_CHECK_EVERY {
                    continue;
                }
                polls_since_check = 0;
                state.recover_apps(&reconnect, auto_launch, token.as_deref()).await;
            }
            _ = relaunch.notified() => {
                state.redeliver_primary_token(token.as_deref()).await;
            }
            serial = extra_relaunch => {
                deliver_extra_token(&serial, token.as_deref(), &mut state.extra_backoff, std::time::Instant::now(), "adb").await;
            }
        }
    }
    futures_util::future::join_all(state.extras.into_values().map(ExtraSession::stop)).await;
}

fn session_ledger() -> Option<runtime::SessionLedger> {
    match runtime::SessionLedger::new(runtime::runtime_dir().join("sessions.json")) {
        Ok(ledger) => Some(ledger),
        Err(error) => {
            warn!("Could not publish tablet sessions: {error}");
            None
        }
    }
}

struct TabletMonitor {
    current: Option<String>,
    identities: std::collections::HashMap<String, String>,
    extra_backoff: std::collections::HashMap<String, RelaunchBackoff>,
    forwarding_backoff: std::collections::HashMap<String, RelaunchBackoff>,
    extras: std::collections::HashMap<String, ExtraSession>,
    // Failed authentication retries grow from 5s to 10 minutes; reconnect resets them.
    last_relaunch: std::time::Instant,
    relaunches: u32,
    relaunch_wait: std::time::Duration,
    wifi_announced: bool,
}

impl TabletMonitor {
    fn new() -> Self {
        Self {
            current: None,
            identities: Default::default(),
            extra_backoff: Default::default(),
            forwarding_backoff: Default::default(),
            extras: Default::default(),
            last_relaunch: std::time::Instant::now() - std::time::Duration::from_secs(60),
            relaunches: 0,
            relaunch_wait: std::time::Duration::from_secs(5),
            wifi_announced: false,
        }
    }

    async fn update_primary(
        &mut self,
        found: &Option<String>,
        ports: (u16, u16),
        auto_launch: bool,
        token: Option<&str>,
        tablet_tx: &watch::Sender<bool>,
    ) {
        let (video_port, input_port) = ports;
        if self.current != *found {
            if let Some(old) = self.current.as_ref() {
                info!("Tablet disconnected or changing transport ({old})");
                let _ = tablet_tx.send(false);
                disconnected_primary(&mut self.current, &mut self.wifi_announced);
            }
            if let Some(serial) = found.as_deref() {
                let request = TabletConnection {
                    serial,
                    video_port,
                    input_port,
                    auto_launch,
                    token,
                    adb: "adb",
                };
                if request
                    .prepare(
                        self.forwarding_backoff
                            .entry(serial.to_string())
                            .or_default(),
                        std::time::Instant::now(),
                    )
                    .await
                {
                    info!(
                        "Tablet connected over {} ({serial})",
                        transport_of(serial).label()
                    );
                    announce_transport(serial);
                    let _ = tablet_tx.send(true);
                    self.current = found.clone();
                    self.extra_backoff
                        .insert(serial.to_string(), RelaunchBackoff::default());
                    self.relaunches = 0;
                    self.relaunch_wait = std::time::Duration::from_secs(5);
                }
            }
        }
    }

    async fn sync_extra_sessions(
        &mut self,
        extra: &ExtraSessionTemplate,
        devices: &[String],
        primary: Option<&str>,
        auto_launch: bool,
        token: Option<&str>,
    ) {
        if extra.max_tablets > 1 {
            // Reserve the selected primary even while forwarding is pending.
            let others = extra_devices(devices, primary);
            self.remove_gone_extras(&others).await;
            self.start_new_extras(extra, others, auto_launch, token)
                .await;
        }
    }

    async fn remove_gone_extras(&mut self, others: &[String]) {
        // Gone
        let gone: Vec<String> = self
            .extras
            .keys()
            .filter(|k| !others.contains(k))
            .cloned()
            .collect();
        for serial in gone {
            self.extra_backoff.remove(&serial);
            if let Some(sess) = self.extras.remove(&serial) {
                info!("Tablet {} disconnected ({})", sess.instance + 1, serial);
                sess.stop().await;
            }
        }
    }

    fn available_slot(&self, max_tablets: u32) -> Option<u32> {
        let used: Vec<u32> = self.extras.values().map(|s| s.instance).collect();
        (1..max_tablets).find(|i| !used.contains(i))
    }

    async fn start_new_extras(
        &mut self,
        extra: &ExtraSessionTemplate,
        others: Vec<String>,
        auto_launch: bool,
        token: Option<&str>,
    ) {
        for serial in others {
            if self.extras.contains_key(&serial)
                || self
                    .forwarding_backoff
                    .get(&serial)
                    .is_some_and(|retry| !retry.ready(std::time::Instant::now()))
            {
                continue;
            }
            let Some(instance) = self.available_slot(extra.max_tablets) else {
                warn!(
                    "Tablet {} attached but all {} slots are taken",
                    serial, extra.max_tablets
                );
                continue;
            };
            info!(
                "Tablet {} connected over {} ({})",
                instance + 1,
                transport_of(&serial).label(),
                serial
            );
            let sess = match spawn_extra_session(extra, instance).await {
                Ok(session) => session,
                Err(error) => {
                    warn!("Could not start tablet {}: {}", instance + 1, error);
                    continue;
                }
            };
            let request = TabletConnection {
                serial: &serial,
                video_port: sess.video_port,
                input_port: sess.input_port,
                auto_launch,
                token,
                adb: "adb",
            };
            if !request
                .prepare(
                    self.forwarding_backoff.entry(serial.clone()).or_default(),
                    std::time::Instant::now(),
                )
                .await
            {
                sess.stop().await;
                continue;
            }
            let _ = sess.tablet_tx.send(true);
            self.extra_backoff
                .insert(serial.clone(), RelaunchBackoff::default());
            self.extras.insert(serial, sess);
        }
    }

    async fn recover_apps(
        &mut self,
        reconnect: &WifiReconnect,
        auto_launch: bool,
        token: Option<&str>,
    ) {
        // Nothing attached, but this tablet has been set up for
        // Wi-Fi: try to get it back. adb answers instantly when the
        // tablet is not reachable, so this costs nothing while it is
        // off or out of range.
        if self.current.is_none() {
            if let Some(address) = reconnect.connect().await {
                if !self.wifi_announced {
                    info!("Reconnected to the tablet over Wi-Fi ({})", address);
                    self.wifi_announced = true;
                }
            }
        }
        let assigned: Vec<_> = self
            .current
            .iter()
            .chain(self.extras.keys())
            .cloned()
            .collect();
        recover_assigned_apps(
            &assigned,
            auto_launch,
            token,
            &mut self.extra_backoff,
            std::time::Instant::now(),
            "adb",
        )
        .await;
    }

    async fn redeliver_primary_token(&mut self, token: Option<&str>) {
        const RELAUNCH_WAIT_MAX: std::time::Duration = std::time::Duration::from_secs(600);
        if let Some(serial) = self.current.as_deref() {
            if self.last_relaunch.elapsed() >= self.relaunch_wait {
                self.last_relaunch = std::time::Instant::now();
                self.relaunches += 1;
                if self.relaunches == 3 {
                    warn!(
                        "A client keeps connecting without a valid token. Update the \
                         UScreen app on the tablet (1.1.0 or newer); the token will be \
                         delivered again, less and less often, until it works."
                    );
                }
                info!(
                    "Delivering the session token to the app (attempt {}, next in {:?})",
                    self.relaunches,
                    self.relaunch_wait.min(RELAUNCH_WAIT_MAX)
                );
                launch_app(serial, token).await;
                self.relaunch_wait = (self.relaunch_wait * 2).min(RELAUNCH_WAIT_MAX);
            }
        }
    }

    fn publish_sessions(
        &self,
        ledger: &mut Option<runtime::SessionLedger>,
        video_port: u16,
        input_port: u16,
    ) {
        if let Some(ledger) = ledger {
            let mut sessions: Vec<_> = self
                .current
                .iter()
                .map(|serial| runtime::TabletSession {
                    serial: serial.clone(),
                    instance: 0,
                    video_port,
                    input_port,
                })
                .collect();
            sessions.extend(
                self.extras
                    .iter()
                    .map(|(serial, session)| runtime::TabletSession {
                        serial: serial.clone(),
                        instance: session.instance,
                        video_port: session.video_port,
                        input_port: session.input_port,
                    }),
            );
            if let Err(error) = ledger.update(sessions) {
                warn!("Could not update tablet sessions: {error}");
            }
        }
    }
}

fn add_fake_tablets(devices: &mut Vec<String>) {
    // Test hook: pretend a serial is attached so a second pipeline can be
    // exercised with one physical tablet and a loopback client.
    if let Ok(fake) = std::env::var("USCREEN_FAKE_TABLET") {
        for f in fake.split(',').map(str::trim).filter(|f| !f.is_empty()) {
            if !devices.iter().any(|d| d == f) {
                devices.push(f.to_string());
            }
        }
    }
}

async fn wait_extra_relaunch(
    requests: Vec<(String, std::sync::Arc<tokio::sync::Notify>)>,
) -> String {
    if requests.is_empty() {
        return std::future::pending::<String>().await;
    }
    let futures = requests.iter().map(|(serial, notify)| {
        Box::pin(async move {
            notify.notified().await;
            serial.clone()
        })
    });
    futures_util::future::select_all(futures).await.0
}

struct RelaunchBackoff {
    next: Option<std::time::Instant>,
    delay: std::time::Duration,
}
impl Default for RelaunchBackoff {
    fn default() -> Self {
        Self {
            next: None,
            delay: std::time::Duration::from_secs(5),
        }
    }
}
impl RelaunchBackoff {
    fn ready(&self, now: std::time::Instant) -> bool {
        self.next.is_none_or(|next| now >= next)
    }

    fn allow(&mut self, now: std::time::Instant) -> bool {
        if !self.ready(now) {
            return false;
        }
        self.next = Some(now + self.delay);
        self.delay = (self.delay * 2).min(std::time::Duration::from_secs(600));
        true
    }
}

async fn deliver_extra_token(
    requested: &str,
    token: Option<&str>,
    policies: &mut std::collections::HashMap<String, RelaunchBackoff>,
    now: std::time::Instant,
    adb: &str,
) {
    if !is_fake_serial(requested)
        && policies
            .get_mut(requested)
            .is_some_and(|policy| policy.allow(now))
    {
        launch_app_using(requested, token, adb).await;
    }
}

async fn recover_assigned_apps(
    assigned: &[String],
    auto_launch: bool,
    token: Option<&str>,
    policies: &mut std::collections::HashMap<String, RelaunchBackoff>,
    now: std::time::Instant,
    adb: &str,
) {
    if !auto_launch {
        return;
    }
    for serial in assigned {
        if is_fake_serial(serial) {
            continue;
        }
        let policy = policies.entry(serial.clone()).or_default();
        match app_running_with(serial, adb).await {
            Some(true) => *policy = RelaunchBackoff::default(),
            Some(false) if policy.allow(now) => launch_app_using(serial, token, adb).await,
            _ => {}
        }
    }
}

struct WifiReconnect {
    path: PathBuf,
    adb: String,
}
impl WifiReconnect {
    fn new(path: PathBuf, adb: String) -> Self {
        Self { path, adb }
    }
    async fn connect(&self) -> Option<String> {
        let address = config::FileConfig::load_at(&self.path).wifi_address;
        if address.is_empty() {
            return None;
        }
        let output = tokio::process::Command::new(&self.adb)
            .args(["connect", &address])
            .output_bounded()
            .await
            .ok()?;
        if config::FileConfig::load_at(&self.path).wifi_address != address {
            // --off or a replacement address may arrive while adb is connecting.
            let _ = tokio::process::Command::new(&self.adb)
                .args(["disconnect", &address])
                .output_bounded()
                .await;
            return None;
        }
        (output.status.success() && String::from_utf8_lossy(&output.stdout).contains("connected"))
            .then_some(address)
    }
}

/// Switch the tablet's adb to TCP and remember where it lives, so the daemon
/// can pick it up over Wi-Fi on its own from then on.
///
/// This is deliberately the adb route rather than a port of our own. The
/// video and input ports stay on loopback, reachable only through the tunnel
/// adb builds, so nothing new is exposed to the network and the tablet still
/// has to be a device this computer is authorised to talk to.
async fn setup_wifi(off: bool) -> Result<()> {
    let cfg = config::FileConfig::load();

    if off {
        config::FileConfig::update(|cfg| {
            cfg.wifi_address.clear();
            Ok(())
        })?;
        if !cfg.wifi_address.is_empty() {
            let _ = tokio::process::Command::new("adb")
                .args(["disconnect", &cfg.wifi_address])
                .output_bounded()
                .await;
        }
        println!("Wi-Fi off. Plug the cable in to use the tablet again.");
        return Ok(());
    }

    let Some(serial) = wifi_device_with(&adb_devices().await, "adb").await else {
        anyhow::bail!(
            "No USB tablet with UScreen installed. Install the app and plug the cable in for this one step — the tablet has to be told \
             to listen on the network, and only the cable can tell it."
        );
    };

    println!("Switching {} to Wi-Fi…", serial);
    let out = tokio::process::Command::new("adb")
        .args(["-s", &serial, "tcpip", "5555"])
        .output_bounded()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "adb tcpip failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    // adbd restarts, taking the USB connection with it for a moment.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let Some(ip) = tablet_ip(&serial).await else {
        anyhow::bail!(
            "The tablet is listening, but its address could not be read. Find it under \
             Settings → About tablet → Status, then put `wifi_address = \"<ip>:5555\"` in \
             ~/.config/uscreen/config.toml."
        );
    };
    let address = format!("{}:5555", ip);

    let out = tokio::process::Command::new("adb")
        .args(["connect", &address])
        .output_bounded()
        .await?;
    let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !said.contains("connected") {
        anyhow::bail!("adb connect {} did not take: {}", address, said);
    }

    config::FileConfig::update(|cfg| {
        cfg.wifi_address = address.clone();
        Ok(())
    })?;
    println!("Connected to {}. The cable can come out.", address);
    println!(
        "The daemon reconnects to this address by itself whenever the cable is not in, \
         so this is a one-off — until the tablet reboots, which puts its adb back on USB \
         and means running this once more.\n\
         Wi-Fi is a fallback: the median latency matches the cable but single frames \
         arrive much later. `uscreen wifi --off` forgets the address."
    );
    Ok(())
}

/// The tablet's own address on the wireless network.
async fn tablet_ip(serial: &str) -> Option<String> {
    // `ip route` is present on every Android that adb can reach, and the
    // route to the default gateway carries the source address we want.
    // Asked for wlan0 first, since a tablet on USB may also have a tethering
    // interface whose address is useless here.
    for args in [
        vec![
            "-s", serial, "shell", "ip", "-f", "inet", "addr", "show", "wlan0",
        ],
        vec!["-s", serial, "shell", "ip", "route", "get", "1.1.1.1"],
        vec!["-s", serial, "shell", "ip", "-f", "inet", "addr"],
    ] {
        let Ok(out) = tokio::process::Command::new("adb")
            .args(&args)
            .output_bounded()
            .await
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(ip) = parse_tablet_ip(&text) {
            return Some(ip);
        }
    }
    None
}

fn parse_tablet_ip(text: &str) -> Option<String> {
    // "inet 192.168.1.42/24 …" or "… src 192.168.1.42 …"
    let mut words = text.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if w != "inet" && w != "src" {
            continue;
        }
        let Some(value) = words.peek() else { continue };
        let ip = value.split('/').next().unwrap_or(value);
        if ip.starts_with("127.") || !ip.contains('.') {
            continue;
        }
        if ip.split('.').count() == 4 && ip.split('.').all(|o| o.parse::<u8>().is_ok()) {
            return Some(ip.to_string());
        }
    }
    None
}

/// Whether the app's process exists on the tablet. `None` when adb could not
/// answer (cable pulled mid-check, adb restarting), so the caller does nothing
/// rather than launching on a guess.
async fn app_running_with(serial: &str, adb: &str) -> Option<bool> {
    let out = tokio::process::Command::new(adb)
        .args(["-s", serial, "shell", "pidof", "com.uscreen"])
        .output_bounded()
        .await
        .ok()?;
    // pidof exits 1 with no output when nothing matches; adb itself failing
    // shows up as a non-empty stderr.
    if !out.stderr.is_empty() {
        return None;
    }
    Some(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// Measured on a quiet network: the median roughly doubles, but the 95th
/// percentile goes from about 28ms to over 150ms and individual frames have
/// been seen at three quarters of a second. Worth saying out loud, because
/// "it works" and "it is pleasant to draw on" are not the same claim.
fn announce_transport(serial: &str) {
    if transport_of(serial) == Transport::Network {
        warn!("Running over Wi-Fi. Expect occasional stutter — the cable is much steadier.");
    }
}

struct TabletConnection<'a> {
    serial: &'a str,
    video_port: u16,
    input_port: u16,
    auto_launch: bool,
    token: Option<&'a str>,
    adb: &'a str,
}

impl TabletConnection<'_> {
    async fn prepare(&self, retry: &mut RelaunchBackoff, now: std::time::Instant) -> bool {
        if is_fake_serial(self.serial) {
            return true;
        }
        if !retry.allow(now) {
            return false;
        }
        match setup_adb_forwarding_with(self.serial, self.video_port, self.input_port, self.adb)
            .await
        {
            Ok(()) => {
                *retry = RelaunchBackoff::default();
                info!(
                    "ADB forwarding ready for {} ({}, {})",
                    self.serial, self.video_port, self.input_port
                );
                if self.auto_launch {
                    launch_app_using(self.serial, self.token, self.adb).await;
                }
                true
            }
            Err(error) => {
                warn!(
                    "ADB forwarding failed for {}: {error}; retry scheduled",
                    self.serial
                );
                false
            }
        }
    }
}

/// Start (or re-front) the app, handing it the session token as an intent
/// extra to the shell-permission-protected TokenActivity, which stores it and
/// fronts the singleTask launcher activity to refresh live connections.
///
/// The command goes to `adb shell` on stdin, not as arguments. Anything in
/// argv is readable by every local process for as long as the adb client
/// runs (/proc/<pid>/cmdline is world-readable), which would hand the token
/// to exactly the attacker it exists to keep out — and a failed auth on the
/// control socket can make the daemon spawn this on demand.
fn app_launch_command(token: Option<&str>) -> String {
    let mut cmd = format!(
        "am start -n com.uscreen/.{}",
        if token.is_some() {
            "TokenActivity"
        } else {
            "MainActivity"
        }
    );
    if let Some(t) = token {
        // Hex only, so no quoting is needed and nothing can break out.
        cmd.push_str(" --es token ");
        cmd.push_str(t);
    }
    cmd.push_str(" >/dev/null 2>&1; exit\n");

    cmd
}

async fn launch_app(serial: &str, token: Option<&str>) {
    launch_app_using(serial, token, "adb").await;
}

async fn launch_app_using(serial: &str, token: Option<&str>, adb: &str) {
    use tokio::io::AsyncWriteExt;
    let cmd = app_launch_command(token);

    let child = tokio::process::Command::new(adb)
        .kill_on_drop(true)
        .args(["-s", serial, "shell"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            warn!("Could not run adb: {}", e);
            return;
        }
    };
    let operation = async {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(cmd.as_bytes()).await?;
            stdin.shutdown().await?;
        }
        child.wait().await
    };
    match tokio::time::timeout(std::time::Duration::from_secs(15), operation).await {
        Ok(Ok(st)) if st.success() => info!("UScreen app launched on tablet"),
        _ => {
            let _ = child.kill().await;
            warn!("Could not launch the app (is it installed?)");
        }
    }
}

/// How the tablet is reached. Nothing in the pipeline is tied to either — it
/// speaks to whatever adb is connected to — but the difference is worth a
/// dozen milliseconds, so it is worth naming.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Transport {
    Usb,
    Network,
}

impl Transport {
    fn label(self) -> &'static str {
        match self {
            Transport::Usb => "USB",
            Transport::Network => "Wi-Fi",
        }
    }
}

/// A network device's serial is its `host:port`; a USB serial never contains a
/// colon. That is the whole distinction adb gives us without a second call.
fn transport_of(serial: &str) -> Transport {
    if serial.contains(':') {
        Transport::Network
    } else {
        Transport::Usb
    }
}

/// Test-only serials supplied through USCREEN_FAKE_TABLET.
fn is_fake_serial(serial: &str) -> bool {
    std::env::var("USCREEN_FAKE_TABLET")
        .map(|f| f.split(',').any(|x| x.trim() == serial))
        .unwrap_or(false)
}

/// Which of the attached devices to drive.
///
/// Stay with the physical tablet already in use; current_transport resolves
/// its preferred USB transport before this function selects a serial.
/// adb lists two devices in no fixed order, and taking `first()` on every
/// poll meant that any reshuffle — a phone's USB re-enumerating when its
/// screen sleeps is enough — looked like a different tablet being plugged in:
/// the port forwards moved, the stream on the real tablet froze on its last
/// frame, and the app went black when it reconnected. That is the shape of
/// the black screen in #10, reported with two Samsung devices attached.
///
/// For a fresh pick, prefer USB over the network, and among USB devices the
/// one that actually has the app installed: a phone charging next to the
/// tablet normally does not, and it is almost never the one meant.
fn current_transport(
    devices: &[String],
    current: Option<&str>,
    identities: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let current = current?;
    if let Some(identity) = identities.get(current) {
        devices
            .iter()
            .find(|device| identities.get(*device) == Some(identity))
            .cloned()
    } else {
        devices
            .iter()
            .find(|device| device.as_str() == current)
            .cloned()
    }
}

async fn unique_devices(
    devices: &[String],
    current: Option<&str>,
    identities: &mut std::collections::HashMap<String, String>,
    adb: &str,
) -> Vec<String> {
    identities.retain(|serial, _| devices.contains(serial) || Some(serial.as_str()) == current);
    let missing = devices
        .iter()
        .filter(|serial| !identities.contains_key(*serial));
    let probes = missing.map(|serial| probe_device_identity(serial, adb));
    for (serial, id) in futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
    {
        identities.insert(serial, id);
    }
    select_device_transports(devices, current, identities)
}

async fn probe_device_identity(serial: &str, adb: &str) -> Option<(String, String)> {
    let output = tokio::process::Command::new(adb)
        .args(["-s", serial, "shell", "getprop", "ro.serialno"])
        .output_bounded()
        .await
        .ok()?;
    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !id.is_empty() && !id.eq_ignore_ascii_case("unknown"))
        .then(|| (serial.to_string(), id))
}

fn select_device_transports(
    devices: &[String],
    current: Option<&str>,
    identities: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mut selected: Vec<String> = Vec::new();
    let mut groups = std::collections::HashMap::<String, usize>::new();
    for serial in devices {
        // Unknown identities remain distinct; never merge unrelated tablets on
        // an empty or failed getprop response.
        let identity = identities
            .get(serial)
            .map(|id| format!("device:{id}"))
            .unwrap_or_else(|| format!("transport:{serial}"));
        if let Some(&index) = groups.get(&identity) {
            let existing = transport_of(&selected[index]);
            let candidate = transport_of(serial);
            if (candidate == Transport::Usb && existing == Transport::Network)
                || (candidate == existing && Some(serial.as_str()) == current)
            {
                selected[index] = serial.clone();
            }
        } else {
            groups.insert(identity, selected.len());
            selected.push(serial.clone());
        }
    }
    selected
}

/// Filter once before assigning any display slot. Explicit test serials do
/// not need a real ADB package manager.
async fn app_devices_with(devices: &[String], adb: &str) -> Vec<String> {
    futures_util::future::join_all(devices.iter().map(|serial| async move {
        let eligible = is_fake_serial(serial)
            || tokio::process::Command::new(adb)
                .args(["-s", serial, "shell", "pm", "path", "com.uscreen"])
                .output_bounded()
                .await
                .map(|out| {
                    out.status.success()
                        && String::from_utf8_lossy(&out.stdout)
                            .lines()
                            .any(|line| line.starts_with("package:"))
                })
                .unwrap_or(false);
        eligible.then(|| serial.clone())
    }))
    .await
    .into_iter()
    .flatten()
    .collect()
}

async fn wifi_device_with(devices: &[String], adb: &str) -> Option<String> {
    let usb: Vec<_> = devices
        .iter()
        .filter(|s| transport_of(s) == Transport::Usb)
        .cloned()
        .collect();
    app_devices_with(&usb, adb).await.into_iter().next()
}

fn select_tablet(devices: &[String], current: Option<&str>) -> Option<String> {
    current
        .filter(|cur| devices.iter().any(|d| d == cur))
        .map(String::from)
        .or_else(|| {
            devices
                .iter()
                .find(|d| transport_of(d) == Transport::Usb)
                .cloned()
        })
        .or_else(|| devices.first().cloned())
}

async fn pick_device_with(devices: &[String], current: Option<&str>, adb: &str) -> Option<String> {
    select_tablet(&app_devices_with(devices, adb).await, current)
}

/// Every device in state "device", USB entries first.
async fn adb_devices() -> Vec<String> {
    let Ok(out) = tokio::process::Command::new("adb")
        .arg("devices")
        .output_bounded()
        .await
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut ready: Vec<String> = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            let state = parts.next()?;
            (state == "device").then(|| serial.to_string())
        })
        .collect();
    ready.sort_by_key(|s| transport_of(s) != Transport::Usb);
    ready
}

/// The ports the tablet app dials on its own loopback. Fixed in the app
/// (VideoReceiver.PORT, TouchCapture.WS_URL), so `adb reverse` maps them per
/// device onto whatever this daemon actually listens on: the configured
/// ports for the first tablet, base + 2 per instance for the others.
const APP_VIDEO_PORT: u16 = 8890;
const APP_INPUT_PORT: u16 = 8891;

async fn setup_adb_forwarding_with(
    serial: &str,
    video_port: u16,
    input_port: u16,
    adb: &str,
) -> Result<()> {
    for (remote, local) in [(APP_VIDEO_PORT, video_port), (APP_INPUT_PORT, input_port)] {
        let remote = format!("tcp:{}", remote);
        let local = format!("tcp:{}", local);
        let r = tokio::process::Command::new(adb)
            .args(["-s", serial, "reverse", &remote, &local])
            .output_bounded()
            .await?;
        if !r.status.success() {
            anyhow::bail!(
                "adb reverse {} {} failed: {}",
                remote,
                local,
                String::from_utf8_lossy(&r.stderr).trim()
            );
        }
    }
    Ok(())
}

async fn stop_pids(pids: &[u32]) -> Result<()> {
    for &pid in pids {
        if config::daemon_is_running(pid) {
            let status = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if status != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while pids.iter().any(|&pid| config::daemon_is_running(pid)) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .context("daemon did not finish shutting down within 10s")?;
    Ok(())
}

fn extra_devices(devices: &[String], primary: Option<&str>) -> Vec<String> {
    devices
        .iter()
        .filter(|serial| Some(serial.as_str()) != primary)
        .cloned()
        .collect()
}

fn disconnected_primary(current: &mut Option<String>, wifi_announced: &mut bool) {
    *current = None;
    *wifi_announced = false;
}

fn forwarding_instructions(video_port: u16, input_port: u16) -> String {
    format!("    adb reverse tcp:{APP_VIDEO_PORT} tcp:{video_port}\n    adb reverse tcp:{APP_INPUT_PORT} tcp:{input_port}")
}

async fn stop_daemon() -> Result<()> {
    stop_daemon_at(&get_pid_path(), other_daemons()).await
}

async fn stop_daemon_at(pid_path: &std::path::Path, mut pids: Vec<u32>) -> Result<()> {
    let uid = unsafe { libc::getuid() };
    pids.retain(|&pid| pid != std::process::id() && is_daemon_process(pid, uid));
    let tracked = std::fs::read_to_string(pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if let Some(pid) = tracked.filter(|&p| p != std::process::id() && is_daemon_process(p, uid)) {
        if !pids.contains(&pid) {
            pids.push(pid);
        }
    }
    stop_pids(&pids).await?;
    if let Some(pid) = tracked {
        remove_pid_file_if_ours(pid_path, pid);
    }
    info!("uscreen daemon stopped");
    Ok(())
}

async fn show_status() -> Result<()> {
    let pid_path = get_pid_path();
    // PID files can be lost, corrupt, or refer to a reused non-daemon PID.
    // Apply the same identity checks as start/stop in every case.
    let pids = other_daemons();
    let tracked = std::fs::read_to_string(&pid_path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok());
    if let Some(stale) = tracked.filter(|pid| !pids.contains(pid)) {
        remove_pid_file_if_ours(&pid_path, stale);
    }
    if pids.is_empty() {
        println!("uscreen is not running");
    } else {
        let pids = pids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        println!("uscreen is running (PID: {})", pids);
    }
    Ok(())
}

async fn list_displays() -> Result<()> {
    println!("=== Available displays ===");
    if let Ok(out) = tokio::process::Command::new("kscreen-doctor")
        .args(["-o"])
        .output_bounded()
        .await
    {
        println!("{}", String::from_utf8_lossy(&out.stdout));
    }

    if let Ok(out) = tokio::process::Command::new("wpctl")
        .args(["status"])
        .output_bounded()
        .await
    {
        println!("--- PipeWire ---");
        println!("{}", String::from_utf8_lossy(&out.stdout));
    }
    Ok(())
}
