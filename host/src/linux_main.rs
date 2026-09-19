mod adb_inventory;
#[cfg(test)]
mod allocation_probe;
#[cfg(not(feature = "inproc-encoder"))]
mod annex_b;
mod attachment;
mod capture;
mod config;
mod desktop;
mod device_tasks;
mod discovery;
mod doctor;
mod edid;
#[cfg(feature = "inproc-encoder")]
mod encoder;
#[cfg_attr(not(feature = "inproc-encoder"), allow(dead_code))]
mod encoder_io;
#[cfg(not(feature = "inproc-encoder"))]
mod framed_annex_b;
mod input;
#[cfg(not(feature = "inproc-encoder"))]
mod ivf;
mod kscreen;
mod kwin;
mod latency;
mod media;
mod media_storage;
mod monitor;
mod osk;
mod persistence;
#[cfg(test)]
mod poll_probe;
mod runtime;
mod selection;
mod session;
mod stream;
mod tray;
mod update;
mod vdisplay;
mod video_queue;

use anyhow::{Context, Result};
use clap::Parser;
#[cfg(test)]
mod discovery_tests;

#[cfg(test)]
use monitor::test_support::{deliver_extra_token, tick_assigned_apps};
#[cfg(test)]
use session::start_servers;
use session::Runtime as ExtraSession;
use std::path::PathBuf;
use tokio::signal;
use tokio::sync::watch;
use tracing::{error, info, warn};
use uscreen_config::adb::{transport_of, Transport};
use uscreen_config::cli::{Cli, Commands};
use uscreen_config::commands::AsyncCommandExt;

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
        conversion_threads: cli.conversion_threads.unwrap_or(saved.conversion_threads),
        video_port: cli.video_port.unwrap_or(saved.video_port),
        input_port: cli.input_port.unwrap_or(saved.input_port),
        pen_only: cli.pen_only || saved.pen_only,
        ..saved.clone()
    };
    effective.sanitize_requested();
    effective
}

#[cfg(test)]
mod cli_tests {
    #[test]
    fn t250_activity_and_token_targets_use_fork_package_and_original_classes() {
        let with_token = super::app_launch_command(Some(&"a".repeat(64)));
        assert!(with_token.contains("io.github.geraldo_netto.uscreen/com.uscreen.TokenActivity"));
        assert!(super::app_launch_command(None)
            .contains("io.github.geraldo_netto.uscreen/com.uscreen.MainActivity"));
        assert!(super::token_delivery_command(None)
            .contains("io.github.geraldo_netto.uscreen/com.uscreen.TokenReceiver"));
    }

    #[tokio::test]
    async fn t250_discovery_requires_the_fork_even_when_upstream_is_installed() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let adb = root.path().join("adb");
        std::fs::write(&adb, r#"#!/bin/sh
if [ "$6" = com.uscreen ]; then echo package:/upstream/base.apk; exit 0; fi
if [ "$6" = io.github.geraldo_netto.uscreen ] && [ "$2" = FORK ]; then echo package:/fork/base.apk; exit 0; fi
exit 1
"#).unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!super::app_installed_with("UPSTREAM", adb.to_str().unwrap()).await);
        assert!(super::app_installed_with("FORK", adb.to_str().unwrap()).await);
    }

    #[test]
    fn t321_explicit_cli_low_clock_is_not_silently_repaired() {
        use clap::Parser;
        let cli = super::Cli::parse_from([
            "uscreen", "--width", "640", "--height", "480", "--fps", "10",
        ]);
        let config = super::effective_config(&cli, &Default::default());
        assert_eq!(config.fps, 10);
        assert!(
            config.validate().is_err(),
            "T321: invalid explicit mode accepted"
        );
    }

    #[test]
    fn t474_cli_overrides_saved_capacity_and_reaches_every_slot() {
        use super::*;
        let saved = config::FileConfig {
            conversion_threads: 64,
            ..Default::default()
        };
        assert_eq!(
            effective_config(&Cli::try_parse_from(["uscreen"]).unwrap(), &saved).conversion_threads,
            64
        );
        for (value, expected) in [("auto", 0), ("1", 1), ("128", 128)] {
            let cli = Cli::try_parse_from(["uscreen", "--conversion-threads", value]).unwrap();
            let effective = effective_config(&cli, &saved);
            assert_eq!(effective.conversion_threads, expected);
            let template = capture::CaptureConfig {
                conversion_threads: effective.conversion_threads,
                ..Default::default()
            };
            for instance in 0..4 {
                assert_eq!(
                    slot_capture_config(template.clone(), instance).conversion_threads,
                    expected
                );
            }
        }
        assert_eq!(
            saved.conversion_threads, 64,
            "T474: CLI must not persist the override"
        );
    }
    #[test]
    fn t474_conversion_capacity_cli_accepts_auto_and_bounds() {
        use super::*;
        for value in ["auto", "0", "1", "64", "128"] {
            assert!(
                Cli::try_parse_from(["uscreen", "--conversion-threads", value]).is_ok(),
                "T474: {value}"
            );
        }
        for value in ["129", "-1", "garbage", "4294967296"] {
            assert!(
                Cli::try_parse_from(["uscreen", "--conversion-threads", value]).is_err(),
                "T474: {value}"
            );
        }
    }
    // Configuration writers are exercised only in child processes with a private XDG tree.
    fn isolated_config_test(name: &str) -> bool {
        if std::env::var_os("USCREEN_CONFIG_TEST_CHILD").is_some() {
            return false;
        }
        let dir = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("USCREEN_CONFIG_TEST_CHILD", "1")
            .env("XDG_CONFIG_HOME", dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        true
    }

    #[tokio::test]
    async fn t332_start_rejects_cli_and_saved_invalid_modes_before_resources() {
        use super::*;
        if isolated_config_test(
            "cli_tests::t332_start_rejects_cli_and_saved_invalid_modes_before_resources",
        ) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
        let path = config::config_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for saved_fps in [60, 90] {
            std::fs::write(
                &path,
                format!("width = 3840\nheight = 2160\nfps = {saved_fps}\n"),
            )
            .unwrap();
            let mut cli =
                Cli::try_parse_from(["uscreen", "--helper", "/nonexistent-t332-helper"]).unwrap();
            if saved_fps == 60 {
                cli.fps = Some(90);
            }
            let error = run_daemon(cli).await.unwrap_err().to_string();
            assert!(error.contains("655.35 MHz"), "T332: {error}");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }

    #[tokio::test]
    async fn t288_start_rejects_cli_and_saved_pen_mode_before_resources() {
        use super::*;
        if isolated_config_test(
            "cli_tests::t288_start_rejects_cli_and_saved_pen_mode_before_resources",
        ) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
        let path = config::config_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for saved_mode in [false, true] {
            std::fs::write(
                &path,
                format!("input_pen = false\npen_only = {saved_mode}\n"),
            )
            .unwrap();
            let mut cli =
                Cli::try_parse_from(["uscreen", "--helper", "/nonexistent-t288-helper"]).unwrap();
            cli.pen_only = !saved_mode;
            let error = run_daemon(cli).await.unwrap_err().to_string();
            assert!(error.contains("requires Pen"), "T288: {error}");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }

    #[cfg(feature = "inproc-encoder")]
    #[tokio::test]
    async fn t284_start_rejects_vaapi_before_runtime_or_helper_setup() {
        use super::*;
        if isolated_config_test(
            "cli_tests::t284_start_rejects_vaapi_before_runtime_or_helper_setup",
        ) {
            return;
        }
        for encoder in [
            "h264_vaapi",
            "h264_vaapi_baseline",
            "hevc_vaapi",
            "vaapih264enc",
        ] {
            for explicit in [false, true] {
                let saved = config::FileConfig {
                    encoder: if explicit { "libx264" } else { encoder }.into(),
                    ..Default::default()
                };
                saved.save().unwrap();
                let mut args = vec!["uscreen", "--helper", "/nonexistent-t284-helper"];
                if explicit {
                    args.extend(["--encoder", encoder]);
                }
                let error = run_daemon(Cli::try_parse_from(args).unwrap())
                    .await
                    .unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("VAAPI is unavailable in this in-process build"),
                    "T284: {error:#}"
                );
            }
        }
    }

    async fn assert_persistence_keeps_runtime_responsive(
        writer: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        config::FileConfig::default().save().unwrap();
        let lock =
            std::fs::File::open(config::config_path().unwrap().with_extension("lock")).unwrap();
        lock.lock().unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _ = wait.recv_timeout(std::time::Duration::from_secs(2));
            drop(lock);
        });
        let started = std::time::Instant::now();
        let task = tokio::spawn(writer);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let elapsed = started.elapsed();
        let _ = release.send(());
        holder.join().unwrap();
        task.await.unwrap();
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "T385: config lock blocked the current-thread runtime for {elapsed:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn t385_mode_persistence_does_not_block_runtime() {
        if isolated_config_test("cli_tests::t385_mode_persistence_does_not_block_runtime") {
            return;
        }
        let (sender, persistence) = mode_channel(false);
        sender.send(true).unwrap();
        drop(sender);
        assert_persistence_keeps_runtime_responsive(persistence.run()).await;
        assert!(config::FileConfig::load().pen_only);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn t385_settings_persistence_does_not_block_runtime() {
        if isolated_config_test("cli_tests::t385_settings_persistence_does_not_block_runtime") {
            return;
        }
        let initial = media::EncoderSettings {
            encoder: "h264_nvenc".into(),
            fps: 60,
            bitrate: 20000,
            width: 1920,
            height: 1080,
            quality: 20,
            width_mm: 310,
            height_mm: 194,
            stream_scale: 1,
            geometry_ready: false,
            decoders: None,
            decoder_epoch: 0,
            selection: None,
        };
        let (sender, receiver) = watch::channel(initial.clone());
        let cli = Cli::try_parse_from(["uscreen"]).unwrap();
        let writer = persist_settings(receiver, CliOverrides::new(&cli));
        sender
            .send(media::EncoderSettings { fps: 30, ..initial })
            .unwrap();
        drop(sender);
        assert_persistence_keeps_runtime_responsive(writer).await;
        assert_eq!(config::FileConfig::load().fps, 30);
    }

    #[tokio::test]
    async fn t297_failed_settings_save_is_retried_on_next_update() {
        if isolated_config_test("cli_tests::t297_failed_settings_save_is_retried_on_next_update") {
            return;
        }
        let saved = config::FileConfig::default();
        saved.save().unwrap();
        let initial = media::EncoderSettings {
            encoder: saved.encoder.clone(),
            fps: saved.fps,
            bitrate: saved.bitrate,
            width: saved.width,
            height: saved.height,
            quality: saved.quality,
            width_mm: 310,
            height_mm: 194,
            stream_scale: saved.stream_scale,
            geometry_ready: false,
            decoders: None,
            decoder_epoch: 0,
            selection: None,
        };
        let (sender, receiver) = watch::channel(initial.clone());
        let cli = Cli::try_parse_from(["uscreen"]).unwrap();
        let worker = persistence::Worker::new(config::storage::ConfigStore::default()).unwrap();
        let writer = persist_settings_with(receiver, CliOverrides::new(&cli), worker.writer());
        tokio::pin!(writer);
        std::fs::write(config::config_path().unwrap(), "invalid = [").unwrap();
        let unsaved = media::EncoderSettings {
            bitrate: 12000,
            ..initial
        };
        sender.send(unsaved.clone()).unwrap();
        assert!(futures_util::poll!(&mut writer).is_pending());
        // T385: prove the first transaction actually failed before repairing
        // the file; polling an asynchronous writer alone cannot establish it.
        worker.writer().barrier().await;
        assert_eq!(
            std::fs::read_to_string(config::config_path().unwrap()).unwrap(),
            "invalid = ["
        );
        config::FileConfig {
            position: "left".into(),
            ..saved.clone()
        }
        .save()
        .unwrap();
        sender
            .send(media::EncoderSettings { fps: 30, ..unsaved })
            .unwrap();
        drop(sender);
        writer.await;
        worker.shutdown().await;
        assert_eq!(
            config::FileConfig::load(),
            config::FileConfig {
                bitrate: 12000,
                fps: 30,
                position: "left".into(),
                ..saved
            },
            "T297: later saves must include earlier unsaved changes"
        );
    }

    #[tokio::test]
    async fn t296_mode_changes_before_writer_start_are_persisted() {
        if isolated_config_test("cli_tests::t296_mode_changes_before_writer_start_are_persisted") {
            return;
        }
        for (saved, initial, changes, expected) in [
            (false, false, &[true][..], true),
            (true, true, &[false][..], false),
            (false, true, &[][..], false), // unchanged temporary --pen-only
            (false, true, &[false, true][..], true), // deliberate round trip
        ] {
            config::FileConfig {
                pen_only: saved,
                ..Default::default()
            }
            .save()
            .unwrap();
            let (sender, persistence) = mode_channel(initial);
            let input_observer = sender.subscribe();
            // Model messages arriving after the control handlers start, before
            // the persistence task is launched. No daemon or devices are opened.
            for &value in changes {
                sender.send(value).unwrap();
            }
            drop(input_observer);
            drop(sender);
            persistence.run().await;
            assert_eq!(
                config::FileConfig::load().pen_only,
                expected,
                "T296: mode changes {changes:?} from {initial} with saved {saved}"
            );
        }
    }

    #[tokio::test]
    async fn t292_queued_first_settings_update_is_persisted() {
        if isolated_config_test("cli_tests::t292_queued_first_settings_update_is_persisted") {
            return;
        }
        let saved = config::FileConfig {
            encoder: "libx264".into(),
            ..Default::default()
        };
        saved.save().unwrap();
        let initial = media::EncoderSettings {
            encoder: "h264_vaapi".into(),
            fps: 60,
            bitrate: 20000,
            width: 1920,
            height: 1080,
            quality: 20,
            width_mm: 310,
            height_mm: 194,
            stream_scale: 1,
            geometry_ready: false,
            decoders: None,
            decoder_epoch: 0,
            selection: None,
        };
        let (tx, rx) = watch::channel(initial.clone());
        let cli = Cli::try_parse_from(["uscreen", "--encoder", "h264_vaapi"]).unwrap();
        let persist = persist_settings(rx, CliOverrides::new(&cli));
        // A GUI edit to an unrelated field must also survive the delayed writer.
        config::FileConfig::update(|cfg| {
            cfg.pen_only = true;
            Ok(())
        })
        .unwrap();
        tx.send(media::EncoderSettings {
            encoder: "h264_nvenc".into(),
            fps: 30,
            bitrate: 12000,
            width: 2560,
            height: 1600,
            quality: 25,
            stream_scale: 2,
            geometry_ready: true,
            decoders: None,
            decoder_epoch: 0,
            selection: None,
            ..initial
        })
        .unwrap();
        drop(tx);
        persist.await;
        let expected = config::FileConfig {
            fps: 30,
            bitrate: 12000,
            width: 2560,
            height: 1600,
            quality: 25,
            stream_scale: 2,
            pen_only: true,
            ..saved
        };
        assert_eq!(
            config::FileConfig::load(),
            expected,
            "T292: a queued first update must not become the persistence baseline"
        );
    }

    #[test]
    fn t200_runtime_persistence_preserves_cli_and_unchanged_fields() {
        let cli =
            Cli::try_parse_from(["uscreen", "--encoder", "h264_vaapi", "--width", "1280"]).unwrap();
        let overrides = CliOverrides::new(&cli);
        let previous = media::EncoderSettings {
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
            decoders: None,
            decoder_epoch: 0,
            selection: None,
        };
        let changed = media::EncoderSettings {
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
    fn t108_t330_extra_slots_do_not_inherit_card_pins() {
        let template = capture::CaptureConfig {
            card: Some(0),
            width: 1280,
            ..capture::CaptureConfig::default()
        };
        for instance in 0..4 {
            let config = slot_capture_config(template.clone(), instance);
            assert_eq!(config.instance, instance);
            assert_eq!(
                config.card, None,
                "automatic allocation uses exclusive helper leases"
            );
            assert_eq!(config.width, template.width);
        }
        // T108's distinct-card and strict-pin guarantees remain exercised by
        // the C lease tests and the integrated T330 concurrent-helper fixture.
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

    // Inter-device completion order is intentionally independent (T390).
    // Keep exact per-device launch counts across every backoff boundary.
    fn sorted_launches(path: &std::path::Path) -> String {
        let log = std::fs::read_to_string(path).unwrap_or_default();
        let mut lines: Vec<_> = log.lines().collect();
        lines.sort_unstable();
        lines.into_iter().map(|line| format!("{line}\n")).collect()
    }

    #[tokio::test]
    async fn t143_t445_process_loss_stays_manual_across_retry_boundaries() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let adb = root.path().join("adb");
        std::fs::write(
            &adb,
            r#"#!/bin/sh
if [ "$1" = devices ]; then printf 'List of devices attached\n'; exit 0; fi
if [ "$1" = version ]; then exit 0; fi
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
        tick_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now,
            adb.to_str().unwrap(),
        )
        .await;
        let log = adb.with_extension("log");
        assert_eq!(sorted_launches(&log), "");
        tick_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(1),
            adb.to_str().unwrap(),
        )
        .await;
        assert_eq!(sorted_launches(&log), "");
        tick_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(5),
            adb.to_str().unwrap(),
        )
        .await;
        assert_eq!(sorted_launches(&log), "");
        std::fs::write(adb.with_extension("DEAD.alive"), "").unwrap();
        tick_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(6),
            adb.to_str().unwrap(),
        )
        .await;
        std::fs::remove_file(adb.with_extension("DEAD.alive")).unwrap();
        tick_assigned_apps(
            &assigned,
            true,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(7),
            adb.to_str().unwrap(),
        )
        .await;
        let expected = "";
        assert_eq!(sorted_launches(&log), expected);
        tick_assigned_apps(
            &assigned,
            false,
            None,
            &mut policies,
            now + std::time::Duration::from_secs(1000),
            adb.to_str().unwrap(),
        )
        .await;
        assert_eq!(sorted_launches(&log), expected);
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

    #[test]
    fn t278_network_serial_forms_do_not_claim_usb_transport() {
        for serial in [
            "192.0.2.1:5555",
            "tablet.local:5555",
            "[2001:db8::1]:5555",
            "adb-43081FDAS000ST-XKzA7F._adb-tls-connect._tcp",
            "tablet._adb._tcp.local.",
        ] {
            assert_eq!(transport_of(serial), Transport::Network, "T278: {serial}");
            assert_eq!(transport_of(serial).label(), "Network ADB");
        }
        assert_eq!(transport_of("8002RH1010011900"), Transport::Usb);
        assert_eq!(transport_of("8002RH1010011900").label(), "USB");
    }

    #[test]
    fn t278_usb_replaces_current_mdns_without_guessing_unknown_identities() {
        for network in [
            "adb-TABLET-nonce._adb-tls-connect._tcp",
            "TABLET._adb._tcp.local.",
        ] {
            let identities = std::collections::HashMap::from([
                (network.into(), "same-tablet".into()),
                ("TABLET".into(), "same-tablet".into()),
            ]);
            for devices in [
                vec![network.into(), "TABLET".into()],
                vec!["TABLET".into(), network.into()],
            ] {
                let chosen = select_device_transports(&devices, Some(network), &identities);
                assert_eq!(
                    chosen,
                    ["TABLET"],
                    "T278: wireless transport kept USB priority"
                );
                assert_eq!(
                    current_transport(&chosen, Some(network), &identities).as_deref(),
                    Some("TABLET")
                );
                assert_eq!(
                    select_device_transports(&devices, Some(network), &Default::default()),
                    devices,
                    "T278: missing physical identity must not merge devices"
                );
            }
        }
    }

    #[tokio::test]
    async fn t278_wifi_setup_never_uses_mdns_as_the_usb_prerequisite() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let adb = root.path().join("adb");
        std::fs::write(&adb, "#!/bin/sh\nprintf '%s\\n' \"$2\" >> \"$0.log\"\necho package:/data/app/com.uscreen/base.apk\n").unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        let network = [
            "adb-TABLET-nonce._adb-tls-connect._tcp",
            "tablet._adb._tcp.local",
            "[2001:db8::1]:5555",
        ]
        .map(String::from);
        assert_eq!(
            wifi_device_with(&network, adb.to_str().unwrap()).await,
            None
        );
        assert!(
            !adb.with_extension("log").exists(),
            "T278: wireless transport probed as USB"
        );
        let mut both = network.to_vec();
        both.push("USB_TABLET".into());
        assert_eq!(
            wifi_device_with(&both, adb.to_str().unwrap())
                .await
                .as_deref(),
            Some("USB_TABLET")
        );
        assert_eq!(
            std::fs::read_to_string(adb.with_extension("log")).unwrap(),
            "USB_TABLET\n"
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
        assert!(
            cmd.contains("io.github.geraldo_netto.uscreen/com.uscreen.TokenActivity --es token")
        );
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

    #[tokio::test]
    async fn t305_shutdown_already_requested_is_not_lost() {
        for requested_before_subscribe in [true, false] {
            let (sender, _) = watch::channel(requested_before_subscribe);
            let receiver = sender.subscribe();
            sender.send_replace(true);
            // Subscribe after the request as run_daemon can do during startup.
            let receiver = if requested_before_subscribe {
                sender.subscribe()
            } else {
                receiver
            };
            tokio::time::timeout(
                std::time::Duration::from_millis(100),
                wait_for_shutdown(receiver),
            )
            .await
            .expect("T305: an existing quit request must not need another notification");
        }
    }

    #[tokio::test]
    async fn t305_shutdown_waits_for_true_or_channel_closure() {
        let (sender, receiver) = watch::channel(false);
        let wait = wait_for_shutdown(receiver);
        tokio::pin!(wait);
        assert!(futures_util::poll!(&mut wait).is_pending());
        sender.send_replace(false);
        assert!(futures_util::poll!(&mut wait).is_pending());
        sender.send_replace(true);
        assert!(futures_util::poll!(&mut wait).is_ready());

        let (sender, receiver) = watch::channel(false);
        drop(sender);
        assert!(futures_util::poll!(Box::pin(wait_for_shutdown(receiver))).is_ready());
    }

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

    #[test]
    fn t281_attachment_identity_requires_proven_transport_equivalence() {
        let identities = std::collections::HashMap::from([
            ("USB-serial".into(), "physical-1".into()),
            ("192.0.2.1:5555".into(), "physical-1".into()),
        ]);
        assert_eq!(
            attachment_identity("USB-serial", &identities),
            attachment_identity("192.0.2.1:5555", &identities)
        );
        assert_ne!(
            attachment_identity("unknown", &identities),
            attachment_identity("physical-1", &identities)
        );
        assert_ne!(
            attachment_identity("physical-1", &identities),
            attachment_identity("USB-serial", &identities)
        );
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
            let (settings_tx, _settings_rx) = watch::channel(media::EncoderSettings {
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
                decoders: None,
                decoder_epoch: 0,
                selection: None,
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
            let (video_tx, _) = crate::video_queue::channel(8, Default::default());
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
        let prepared = session::Spec {
            capture: Default::default(),
            ports: (0, 0),
            token: None,
            devices: (false, false, false),
        }
        .prepare(watch::channel(false).0);
        let tablet_tx = prepared.tablet;
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

async fn run_daemon(cli: Cli) -> Result<()> {
    // Validate the complete effective range before claiming resources.
    let file_cfg = config::FileConfig::load();
    let effective = effective_config(&cli, &file_cfg);
    effective.validate()?;
    config::validate_encoder_for_build(&effective.encoder)?;

    runtime::runtime_dir().context("validate private runtime directory")?;
    let helper_path = find_helper(cli.helper.as_deref())?;
    let pid_path = get_pid_path();
    if let Some(parent) = pid_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    ensure_single_daemon(&pid_path)?;

    // Write PID file for clean stop/status
    let pid = std::process::id();
    std::fs::write(&pid_path, pid.to_string())?;

    heal_config(&file_cfg);
    uscreen_config::linux::pipe::publish_current()?;
    let encoder = effective.encoder.clone();
    let fps = effective.fps;
    let bitrate = effective.bitrate;
    let width = effective.width;
    let height = effective.height;
    let video_port = effective.video_port;
    let input_port = effective.input_port;
    let quality = effective.quality;
    let stream_scale = effective.stream_scale;
    let pen_only = effective.pen_only;

    let cap_config = slot_capture_config(
        capture::CaptureConfig {
            helper_path: helper_path.clone(),
            profile_cache: file_cfg.profile_cache,
            edid_path: cli.edid.clone(),
            encoder: encoder.clone(),
            decoder: None,
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
            conversion_threads: effective.conversion_threads,
            position: config::Position::parse_or_default(&file_cfg.position),
            ten_bit: file_cfg.ten_bit,
            instance: 0,
            // The helper atomically leases a free card; card order is not ownership.
            card: None,
        },
        0,
    );

    let token = create_session_token(file_cfg.require_token)?;
    let token_dir = token.as_ref().map(|_| runtime::runtime_dir()).transpose()?;
    let (mode_tx, mode_persistence) = mode_channel(pen_only);
    let prepared = session::Spec {
        capture: cap_config.clone(),
        ports: (video_port, input_port),
        token: token.clone(),
        devices: (
            file_cfg.input_touch,
            file_cfg.input_pen,
            file_cfg.input_pointer,
        ),
    }
    .prepare(mode_tx.clone());
    let settings_rx = prepared.settings.subscribe();
    let tablet_tx = prepared.tablet.clone();
    let tray_tablet_rx = tablet_tx.subscribe();
    let relaunch = prepared.relaunch.clone();
    // Snapshot before any producer runs; scheduling the writer can come later.
    // CLI overrides remain temporary and must never be written back.
    let persistence = persistence::Worker::new(config::storage::ConfigStore::default())?;
    let save_settings = persist_settings_with(
        settings_rx.clone(),
        CliOverrides::new(&cli),
        persistence.writer(),
    );

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

    let (shutdown_tx, _) = watch::channel(false);
    if pen_only {
        info!("Starting in pen-only mode; no virtual display or encoding until requested");
    }
    let primary = prepared.start(shutdown_tx.subscribe()).await?;

    let save_handle = tokio::spawn(save_settings);

    // Remember which mode the tablet was left in. Unlike the --pen-only flag,
    // which is a one-off for this run and never written back, a switch made
    // from the tablet is a deliberate choice and should survive a restart.
    let mode_save_handle = tokio::spawn(persist_mode(mode_persistence.0, persistence.writer()));

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
    let extra = ExtraSessionTemplate {
        max_tablets: file_cfg.max_tablets,
        cap_template: capture::CaptureConfig {
            edid_path: None,
            ..cap_config
        },
        video_port,
        input_port,
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
            token_dir,
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
    let quit_rx = shutdown_tx.subscribe();
    tokio::select! {
        _ = signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
        _ = wait_for_shutdown(quit_rx) => {}
    }
    info!("Shutting down...");

    // Ask the capture pipeline to wind down, and give it a bounded moment to
    // actually reap its children before pulling the rug out.
    let _ = shutdown_tx.send(true);
    if tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let _ = tokio::join!(primary.stop(), &mut adb_handle);
    })
    .await
    .is_err()
    {
        warn!("Capture pipelines did not stop within 5s");
        adb_handle.abort();
    }

    // The input server restores the keyboard when the last tablet detaches;
    // this covers a shutdown with a tablet still attached.
    osk::restore().await;

    adb_handle.abort();
    save_handle.abort();
    mode_save_handle.abort();
    let _ = tokio::join!(save_handle, mode_save_handle);
    persistence.shutdown().await;
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

async fn wait_for_shutdown(mut quit_rx: watch::Receiver<bool>) {
    // A tray request can precede subscription; the current flag is authoritative.
    while !*quit_rx.borrow_and_update() {
        if quit_rx.changed().await.is_err() {
            break;
        }
    }
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
    let raw: Option<config::FileConfig> = config::config_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
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

fn persist_settings_with(
    mut settings_rx: watch::Receiver<media::EncoderSettings>,
    cli_overrides: CliOverrides,
    writer: persistence::Writer,
) -> impl std::future::Future<Output = ()> + Send {
    // Capture the baseline at construction, even if this future is polled
    // after a tablet has already published its first settings change.
    let mut previous = settings_rx.borrow().clone();
    async move {
        while settings_rx.changed().await.is_ok() {
            let s = settings_rx.borrow().clone();
            let next = s.clone();
            let baseline = previous.clone();
            let result = writer
                .update(move |cfg| {
                    cli_overrides.apply_encoder(cfg, &next, &baseline);
                    cli_overrides.apply_geometry(cfg, &next, &baseline);
                    Ok(())
                })
                .await;
            if let Err(e) = result {
                warn!("Failed to persist settings: {}", e);
            } else {
                // Failed edits remain pending for the next update/retry.
                previous = s;
                info!("Settings saved to {:?}", config::config_path());
            }
        }
    }
}

#[derive(Clone, Copy)]
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
        s: &media::EncoderSettings,
        previous: &media::EncoderSettings,
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
        s: &media::EncoderSettings,
        previous: &media::EncoderSettings,
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

// Subscribe at channel creation so server startup cannot hide an early switch.
// Hold the receiver until startup succeeds and the writer task can be launched.
struct ModePersistence(watch::Receiver<bool>);

fn mode_channel(initial: bool) -> (watch::Sender<bool>, ModePersistence) {
    let (sender, receiver) = watch::channel(initial);
    (sender, ModePersistence(receiver))
}

#[cfg(test)]
impl ModePersistence {
    fn run(self) -> impl std::future::Future<Output = ()> + Send {
        let worker = persistence::Worker::new(config::storage::ConfigStore::default()).unwrap();
        async move {
            persist_mode(self.0, worker.writer()).await;
            worker.shutdown().await;
        }
    }
}

async fn persist_mode(mut mode_rx: watch::Receiver<bool>, writer: persistence::Writer) {
    while mode_rx.changed().await.is_ok() {
        let pen_only = *mode_rx.borrow();
        if let Err(e) = writer
            .update(move |cfg| {
                cfg.pen_only = pen_only;
                Ok(())
            })
            .await
        {
            warn!("Failed to persist mode: {}", e);
        }
    }
}

#[cfg(test)]
fn persist_settings(
    settings_rx: watch::Receiver<media::EncoderSettings>,
    cli: CliOverrides,
) -> impl std::future::Future<Output = ()> + Send {
    let worker = persistence::Worker::new(config::storage::ConfigStore::default()).unwrap();
    let save = persist_settings_with(settings_rx, cli, worker.writer());
    async move {
        save.await;
        worker.shutdown().await;
    }
}

/// Recover same-user daemons even when their PID file is missing.
fn other_daemons() -> Vec<u32> {
    uscreen_config::linux::daemon::discover(None)
}

fn is_daemon_process(pid: u32, uid: u32) -> bool {
    uscreen_config::linux::daemon::is_daemon_process(pid, uid)
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
/// tablet on demand. Every slot uses the shared session runtime; only the
/// primary session participates in daemon configuration persistence.
struct ExtraSessionTemplate {
    max_tablets: u32,
    cap_template: capture::CaptureConfig,
    video_port: u16,
    input_port: u16,
    token: Option<String>,
    /// Which virtual input devices each extra tablet gets; same switches as
    /// the first one.
    input_touch: bool,
    input_pen: bool,
    input_pointer: bool,
    mode_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

/// Each automatic session has its own FIFO and acquires an exclusive helper
/// lease when capture starts. A template's explicit pin cannot be shared by slots.
fn slot_capture_config(
    mut config: capture::CaptureConfig,
    instance: u32,
) -> capture::CaptureConfig {
    config.instance = instance;
    config.card = None;
    config
}

/// Bring up capture, stream and input for tablet number `instance`.
/// Ports are the base ports plus 2 per instance; the tablet side keeps
/// using 8890/8891, since `adb reverse` maps them per device.
async fn spawn_extra_session(t: &ExtraSessionTemplate, instance: u32) -> Result<ExtraSession> {
    let (video_port, input_port) = *config::slot_ports(t.video_port, t.input_port, t.max_tablets)?
        .get(instance as usize)
        .context("tablet slot outside configured range")?;
    let cfg = slot_capture_config(t.cap_template.clone(), instance);

    let prepared = session::Spec {
        capture: cfg.clone(),
        ports: (video_port, input_port),
        token: t.token.clone(),
        devices: (t.input_touch, t.input_pen, t.input_pointer),
    }
    .prepare(t.mode_tx.clone());
    let runtime = prepared.start(t.shutdown_rx.clone()).await?;
    info!(
        "Tablet slot {} ready: video port {}, input port {}{}",
        instance + 1,
        video_port,
        input_port,
        cfg.card
            .map(|c| format!(", EVDI card{}", c))
            .unwrap_or_default()
    );
    Ok(runtime)
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
    tablet_tx: attachment::Attachment,
    token_dir: Option<PathBuf>,
    relaunch: std::sync::Arc<tokio::sync::Notify>,
    extra: ExtraSessionTemplate,
) {
    adb_monitor_using(
        video_port,
        input_port,
        auto_launch,
        tablet_tx,
        token_dir,
        relaunch,
        extra,
        "adb",
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn adb_monitor_using(
    video_port: u16,
    input_port: u16,
    auto_launch: bool,
    tablet_tx: attachment::Attachment,
    token_dir: Option<PathBuf>,
    relaunch: std::sync::Arc<tokio::sync::Notify>,
    extra: ExtraSessionTemplate,
    adb: &str,
) {
    monitor::run(monitor::Config {
        ports: (video_port, input_port),
        auto_launch,
        tablet: tablet_tx,
        token_dir,
        relaunch,
        extra,
        adb: adb.to_owned(),
    })
    .await;
}

fn session_ledger() -> Option<runtime::SessionLedger> {
    match runtime::runtime_dir()
        .and_then(|dir| runtime::SessionLedger::new(dir.join("sessions.json")))
    {
        Ok(ledger) => Some(ledger),
        Err(error) => {
            warn!("Could not publish tablet sessions: {error}");
            None
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
/// host video and input ports stay on loopback. adb tcpip opens the tablet
/// listener on port 5555; an authorized adb connection carries the tunnel.
/// --off forgets/disconnects that address but does not disable the listener.
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
         Wi-Fi is a fallback: a historical test with the radio lock had a median \
         close to USB but multi-second outliers. Your network may differ. `uscreen wifi --off` forgets the address."
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

/// Historical upstream test (docs/benchmarks.md): median 32.0 ms without
/// the radio lock, 22.8 ms with it, versus 22.0 ms over USB. Locked Wi-Fi
/// still had a 78.6 ms p95 and multi-second outliers. Other networks differ.
fn announce_transport(serial: &str) {
    if transport_of(serial) == Transport::Network {
        warn!("Using network ADB. USB is preferred when both transports identify the same tablet.");
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
/// to a local process that must not receive it. Failed input authentication
/// requests a protected broadcast, never an Activity launch.
fn app_launch_command(token: Option<&str>) -> String {
    use uscreen_config::android::Component;
    let component = if token.is_some() {
        Component::TokenActivity
    } else {
        Component::MainActivity
    };
    let mut cmd = format!("am start -n {}", component.adb_name());
    if let Some(t) = token {
        // Hex only, so no quoting is needed and nothing can break out.
        cmd.push_str(" --es token ");
        cmd.push_str(t);
    }
    cmd.push_str(" >/dev/null 2>&1; exit\n");

    cmd
}

async fn launch_app_using(serial: &str, token: Option<&str>, adb: &str) {
    app_command_using(serial, app_launch_command(token), "launch", adb).await;
}

fn token_delivery_command(token: Option<&str>) -> String {
    let mut command = format!(
        "am broadcast -n {}",
        uscreen_config::android::Component::TokenReceiver.adb_name()
    );
    if let Some(token) = token {
        command.push_str(" --es token ");
        command.push_str(token);
    }
    command.push_str(" >/dev/null 2>&1; exit\n");
    command
}

async fn redeliver_token_using(serial: &str, token: Option<&str>, adb: &str) -> bool {
    app_command_using(serial, token_delivery_command(token), "token delivery", adb).await
}

async fn app_command_using(serial: &str, cmd: String, action: &str, adb: &str) -> bool {
    use tokio::io::AsyncWriteExt;

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
            return false;
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
        Ok(Ok(st)) if st.success() => {
            info!("UScreen {action} command completed on tablet");
            true
        }
        _ => {
            let _ = child.kill().await;
            warn!("Could not complete UScreen {action} (is the matching app installed?)");
            false
        }
    }
}

/// Test-only serials supplied through USCREEN_FAKE_TABLET.
fn is_fake_serial(serial: &str) -> bool {
    std::env::var("USCREEN_FAKE_TABLET")
        .map(|f| f.split(',').any(|x| x.trim() == serial))
        .unwrap_or(false)
}

fn attachment_identity(
    serial: &str,
    identities: &std::collections::HashMap<String, String>,
) -> String {
    match identities.get(serial) {
        Some(identity) => format!("device:{identity}"),
        None => format!("transport:{serial}"),
    }
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
    let missing: Vec<_> = devices
        .iter()
        .filter(|serial| !identities.contains_key(*serial))
        .collect();
    use futures_util::StreamExt;
    let mut probes = futures_util::stream::iter(
        missing
            .into_iter()
            .map(|serial| probe_device_identity(serial, adb)),
    )
    .buffer_unordered(4);
    while let Some(result) = probes.next().await {
        if let Some((serial, id)) = result {
            identities.insert(serial, id);
        }
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
async fn app_installed_with(serial: &str, adb: &str) -> bool {
    app_presence_with(serial, adb).await == Some(true)
}

/// Unknown package observations preserve existing assignments (T413), but
/// cannot admit a newly discovered transport without a successful probe.
async fn app_presence_with(serial: &str, adb: &str) -> Option<bool> {
    if is_fake_serial(serial) {
        return Some(true);
    }
    let out = tokio::process::Command::new(adb)
        .args([
            "-s",
            serial,
            "shell",
            "pm",
            "path",
            uscreen_config::android::PACKAGE,
        ])
        .output_bounded()
        .await
        .ok()?;
    package_presence(&out)
}

fn package_presence(out: &std::process::Output) -> Option<bool> {
    if !out.stderr.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(&out.stdout).ok()?.trim();
    // Android PackageManagerShellCommand.displayPackageFilePath returns 1
    // with empty output when absent. Older adapters also return 0/empty.
    if text.is_empty() && matches!(out.status.code(), Some(0 | 1)) {
        return Some(false);
    }
    if out.status.success() && text.lines().all(|line| line.starts_with("package:/")) {
        return Some(true);
    }
    None
}

async fn app_devices_with(devices: &[String], adb: &str) -> Vec<String> {
    use futures_util::StreamExt;
    let work: Vec<_> = devices
        .iter()
        .cloned()
        .map(|serial| {
            let adb = adb.to_owned();
            async move { app_installed_with(&serial, &adb).await.then_some(serial) }
        })
        .collect();
    futures_util::stream::iter(work)
        .buffered(4)
        .collect::<Vec<_>>()
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
    adb_devices_using("adb").await
}

async fn adb_devices_using(adb: &str) -> Vec<String> {
    adb_inventory::query(adb).await.unwrap_or_default()
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
    tokio::time::timeout(config::commands::DAEMON_STOP_TIMEOUT, async {
        while pids.iter().any(|&pid| config::daemon_is_running(pid)) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .with_context(|| {
        format!(
            "daemon did not finish shutting down within {}s",
            config::commands::DAEMON_STOP_TIMEOUT.as_secs()
        )
    })?;
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
