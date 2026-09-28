//! T494/T524: lifecycle readiness never implies display/input backend support.
#![cfg(windows)]
use blent_config::commands::SyncCommandExt;
use std::process::{Command, Output};
use std::time::Duration;

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blent"))
        .args(args)
        .output_timeout(Duration::from_secs(8))
        .unwrap()
}

#[test]
fn t494_help_and_version_are_available() {
    for arg in ["--help", "--version"] {
        let result = invoke(&[arg]);
        assert!(result.status.success());
        assert!(String::from_utf8_lossy(&result.stdout).contains("blent"));
    }
}

#[test]
fn t494_windows_actions_report_unsupported_without_side_effects() {
    let root = tempfile::tempdir().unwrap();
    for args in [vec!["list-displays"]] {
        let result = Command::new(env!("CARGO_BIN_EXE_blent"))
            .args(args)
            .current_dir(root.path())
            .env("XDG_RUNTIME_DIR", root.path())
            .output_timeout(Duration::from_secs(8))
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported on Windows"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn t494_doctor_distinguishes_configuration_from_backend_support() {
    let result = invoke(&["doctor"]);
    assert!(result.status.success());
    let text = String::from_utf8_lossy(&result.stdout);
    for expected in [
        "Configuration:",
        "ADB:",
        "FFmpeg:",
        "Blent version:",
        "Display: unavailable",
        "Input: unavailable",
        "Tablet connection: unverified",
        "Encoder/decoder compatibility: unverified",
    ] {
        assert!(text.contains(expected), "T494: {text}");
    }
}

#[test]
fn t494_cli_rejects_malformed_and_out_of_bounds_values() {
    for bad in ["-1", "129", "4294967296", "NaN", "", "1.5"] {
        let result = invoke(&["--conversion-threads", bad]);
        assert_eq!(result.status.code(), Some(2), "T494: {bad:?}");
    }
    for bad in ["65536", "-1", "18446744073709551616"] {
        assert_eq!(invoke(&["--video-port", bad]).status.code(), Some(2));
    }
}

#[test]
fn t694_windows_rejects_unavailable_overrides_before_starting() {
    for args in [
        vec!["--edid", "missing.bin"],
        vec!["--helper", "missing.exe"],
        vec!["--encoder", "libx264"],
        vec!["--fps", "30"],
        vec!["--bitrate", "10000"],
        vec!["--width", "1280"],
        vec!["--height", "720"],
        vec!["--quality", "20"],
        vec!["--stream-scale", "2"],
        vec!["--conversion-threads", "1"],
        vec!["--encoder-workers", "1"],
        vec!["--pen-only"],
    ] {
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("runtime");
        let result = Command::new(env!("CARGO_BIN_EXE_blent"))
            .arg("--runtime-dir")
            .arg(&runtime)
            .args(&args)
            .arg("start")
            .output_timeout(Duration::from_secs(2))
            .expect("T694: unsupported option must reject before daemon startup");
        assert!(!result.status.success(), "T694: {args:?}");
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains(args[0]) && error.contains("unavailable"),
            "T694: {error}"
        );
        assert!(
            !runtime.exists(),
            "T694: rejection must not create runtime state"
        );
    }
}

#[test]
fn t694_connection_overrides_require_direct_start() {
    for command in ["status", "stop", "doctor", "--login"] {
        let root = tempfile::tempdir().unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_blent"))
            .arg("--runtime-dir")
            .arg(root.path().join("runtime"))
            .args(["--video-port", "19320", command])
            .output_timeout(Duration::from_secs(2))
            .unwrap();
        assert!(
            !result.status.success(),
            "T694: {command} silently ignored port override"
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("direct daemon start"));
    }
}

#[test]
fn t691_wifi_cli_admits_setup_and_parses_off_without_mutating_settings() {
    let root = tempfile::tempdir().unwrap();
    use clap::Parser;
    let cli = blent_config::cli::Cli::try_parse_from(["blent", "wifi", "--off"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(blent_config::cli::Commands::Wifi { off: true })
    ));
    for args in [vec!["wifi"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_blent"))
            .args(args)
            .env("PATH", root.path())
            .current_dir(root.path())
            .output_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("adb.exe missing from PATH"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
