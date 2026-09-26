//! T494: compilation is a diagnostics milestone, never a working backend claim.
#![cfg(windows)]
use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blent"))
        .args(args)
        .output()
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
    for args in [
        vec![],
        vec!["start"],
        vec!["stop"],
        vec!["status"],
        vec!["list-displays"],
        vec!["wifi"],
        vec!["wifi", "--off"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_blent"))
            .args(args)
            .current_dir(root.path())
            .env("XDG_RUNTIME_DIR", root.path())
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("not implemented on Windows"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn t494_doctor_distinguishes_configuration_from_backend_support() {
    let result = invoke(&["doctor"]);
    assert!(!result.status.success());
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
