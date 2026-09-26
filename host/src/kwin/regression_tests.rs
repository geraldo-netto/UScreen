//! T512: D-Bus scalar parsing must not invent a different keyboard setting.
use super::*;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn t512_qdbus_preserves_signed_scalar_values() {
    if let Ok(expected) = std::env::var("BLENT_T512_EXPECTED") {
        assert_eq!(
            get_property("/VirtualKeyboard", PROBE_IFACE, "mode").await,
            Some(expected)
        );
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let tool = directory.path().join("qdbus");
    std::fs::write(&tool, "#!/bin/sh\nprintf '%s\\n' \"$BLENT_T512_REPLY\"\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    for value in ["-1", "-2147483648", "2147483647", "0", "1", "2", "3", "+1"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "kwin::regression_tests::t512_qdbus_preserves_signed_scalar_values",
                "--nocapture",
            ])
            .env("PATH", directory.path())
            .env("BLENT_T512_REPLY", format!("[Variant(int): {value}]"))
            .env("BLENT_T512_EXPECTED", value)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T512 {value}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
