//! T513: diagnostics must not manufacture a supported mode from malformed replies.
use super::*;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn t515_failed_diagnostic_commands_cannot_supply_successful_settings() {
    let directory = tempfile::tempdir().unwrap();
    let tool = directory.path().join("fake-adb");
    std::fs::write(&tool, "#!/bin/sh\nprintf '0\\n'\nexit \"$1\"\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    let program = tool.to_str().unwrap();
    assert_eq!(
        output_of(program, &["17"]).await,
        None,
        "T515: failed commands must not report apparently valid setting values"
    );
    assert_eq!(output_of(program, &["0"]).await.as_deref(), Some("0\n"));
}

#[tokio::test]
async fn t513_keyboard_report_requires_an_exact_supported_mode() {
    if let Ok(expected) = std::env::var("BLENT_T513_WARNINGS") {
        let mut report = Report::new();
        check_kwin_input(&mut report).await;
        assert_eq!(report.warnings, expected.parse::<u32>().unwrap());
        assert_eq!(report.failures, 0);
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let tool = directory.path().join("busctl");
    std::fs::write(&tool, "#!/bin/sh\nprintf 'i %s\\n' \"$BLENT_T513_MODE\"\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    for (mode, warnings) in [
        ("-1", 0),
        ("mode2", 0),
        ("-2147483648", 0),
        ("3", 0),
        ("", 0),
        ("0", 0),
        ("1", 1),
        ("2", 1),
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::regression_tests::t513_keyboard_report_requires_an_exact_supported_mode",
                "--nocapture",
            ])
            .env("PATH", directory.path())
            .env("BLENT_T513_MODE", mode)
            .env("BLENT_T513_WARNINGS", warnings.to_string())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T513 {mode}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
