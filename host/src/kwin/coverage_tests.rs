//! T497: both command adapters and keyboard lifetime run against a private fake bus.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const TEST: &str = "kwin::coverage_tests::t497_kwin_adapters_restore_only_the_owned_keyboard_state";

const COMMAND: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/calls"
case "$*" in
  *available*) printf 'b true\n';;
  *set-property*|*Properties.Set*)
    for value do :; done
    printf '%s\n' "$value" > "$HOME/mode";;
  *mode*)
    read -r value < "$HOME/mode"
    if [ "$USCREEN_T497_BACKEND" = busctl ]; then printf 'i %s\n' "$value"
    else printf '[Variant(int): %s]\n' "$value"; fi;;
  *bad*) exit 42;;
  *) /bin/cat "$HOME/reply";;
esac
"#;

fn isolated(backend: &str) {
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let tool = directory.path().join(backend);
    std::fs::write(&tool, COMMAND).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(directory.path().join("mode"), "2\n").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env("USCREEN_T497_BACKEND", backend)
        .env("PATH", directory.path())
        .env("HOME", directory.path())
        .env("XDG_RUNTIME_DIR", directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = std::fs::read_to_string(directory.path().join("calls")).unwrap();
    assert!(calls.contains("mode"));
    assert!(calls.contains("fixtureDevices"));
}

async fn properties(root: &Path, busctl: bool) {
    let response = root.join("reply");
    let text = if busctl {
        "s \"DVI-I-1\""
    } else {
        "[Variant(QString): \"DVI-I-1\"]"
    };
    std::fs::write(&response, text).unwrap();
    assert_eq!(
        get_property("/fixture", "fixture", "name").await.as_deref(),
        Some("DVI-I-1")
    );
    let text = if busctl {
        "as 2 \"event7\" \"event8\""
    } else {
        "event7\nevent8\n"
    };
    std::fs::write(&response, text).unwrap();
    assert_eq!(
        list_strings("/fixture", "fixture", "fixtureDevices")
            .await
            .unwrap(),
        ["event7", "event8"]
    );
    std::fs::write(&response, "").unwrap();
    assert!(list_strings("/fixture", "fixture", "fixtureDevices")
        .await
        .unwrap()
        .is_empty());
    assert!(get_property("/bad", "fixture", "name").await.is_none());
    assert!(list_strings("/bad", "fixture", "fixtureDevices")
        .await
        .is_none());
}

async fn keyboard(root: &Path) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let devices = AtomicUsize::new(1);
    crate::osk::sync_touch_state(&devices).await;
    let backup = root.join(".local/share/uscreen/osk-restore");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "2");
    assert_eq!(std::fs::read_to_string(root.join("mode")).unwrap(), "0\n");
    devices.store(0, Ordering::SeqCst);
    crate::osk::sync_touch_state(&devices).await;
    assert_eq!(std::fs::read_to_string(root.join("mode")).unwrap(), "2\n");
    assert!(!backup.exists());
    crate::osk::restore().await;
    assert!(!backup.exists());
}

#[tokio::test]
async fn t497_kwin_adapters_restore_only_the_owned_keyboard_state() {
    let Ok(kind) = std::env::var("USCREEN_T497_BACKEND") else {
        for backend in ["busctl", "qdbus", "qdbus6", "qdbus-qt6", "qdbus-qt5"] {
            isolated(backend);
        }
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    assert_eq!(backend().await.unwrap().name(), kind);
    properties(&root, kind == "busctl").await;
    keyboard(&root).await;
    assert!(set_property("/fixture", "fixture", "mode", "i", "1").await);
    assert_eq!(
        get_property("/fixture", "fixture", "mode").await.as_deref(),
        Some("1")
    );
}
