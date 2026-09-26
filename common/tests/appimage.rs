//! T308: isolate launcher environment and service probes from the login session.
#![cfg(all(target_os = "linux", feature = "platform"))]
use blent_config::linux::appimage;
use std::{os::unix::fs::PermissionsExt, process::Command};

#[test]
fn t308_service_binding_child() {
    let Ok(expected) = std::env::var("BLENT_T308_EXPECT") else {
        return;
    };
    assert_eq!(appimage::permits_service(), expected == "true");
}

#[test]
fn t308_services_match_only_the_current_stable_distribution() {
    let root = tempfile::tempdir().unwrap();
    let image = root.path().join("path space % dollar$.AppImage");
    std::fs::write(&image, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o700)).unwrap();
    let systemctl = root.path().join("systemctl");
    std::fs::write(
        &systemctl,
        "#!/bin/sh\nprintf '%s\\n' \"$BLENT_T308_UNIT\"\nexit \"$BLENT_T308_EXIT\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&systemctl, std::fs::Permissions::from_mode(0o700)).unwrap();
    let unit = root.path().join("blent.service");
    for (launcher, text, exit, expected) in [
        (None, String::new(), "0", true),
        (Some(image.as_path()), appimage::marker(&image), "0", true),
        (Some(image.as_path()), appimage::marker(&image), "1", false),
        (
            Some(image.as_path()),
            "[Service]\nExecStart=/usr/bin/blent".into(),
            "0",
            false,
        ),
        (
            Some(std::path::Path::new("relative")),
            String::new(),
            "0",
            false,
        ),
    ] {
        std::fs::write(&unit, text).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "t308_service_binding_child", "--nocapture"])
            .env("PATH", root.path())
            .env("BLENT_T308_UNIT", &unit)
            .env("BLENT_T308_EXIT", exit)
            .env("BLENT_T308_EXPECT", expected.to_string())
            .env_remove(appimage::LAUNCHER);
        if let Some(launcher) = launcher {
            command.env(appimage::LAUNCHER, launcher);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn t308_autostart_child() {
    let Ok(mode) = std::env::var("BLENT_T308_AUTOSTART") else {
        return;
    };
    let binary = std::path::Path::new("/fixture/blent.AppImage");
    use blent_config::linux::autostart;
    match mode.as_str() {
        "managed" => {
            assert!(autostart::systemd_available());
            autostart::set_enabled(true, binary).unwrap();
            autostart::set_enabled(false, binary).unwrap();
            assert!(!autostart::desktop_path().unwrap().exists());
        }
        "direct" => {
            assert!(!autostart::enabled());
            autostart::set_enabled(true, binary).unwrap();
            assert!(autostart::enabled());
            assert!(std::fs::read_to_string(autostart::desktop_path().unwrap())
                .unwrap()
                .contains("/fixture/blent.AppImage"));
            autostart::set_enabled(false, binary).unwrap();
            assert!(!autostart::enabled());
        }
        "other" => assert!(autostart::set_enabled(true, binary)
            .unwrap_err()
            .to_string()
            .contains("Another Blent distribution")),
        "unreachable" => assert!(autostart::set_enabled(true, binary)
            .unwrap_err()
            .to_string()
            .contains("manager is unreachable")),
        "denied" => assert!(autostart::set_enabled(true, binary)
            .unwrap_err()
            .to_string()
            .contains("systemctl enable failed")),
        _ => panic!("unknown fixture"),
    }
}

#[test]
fn t308_registration_controls_only_its_service_and_preserves_direct_autostart() {
    for mode in ["managed", "direct", "other", "unreachable", "denied"] {
        let root = tempfile::tempdir().unwrap();
        let image = root.path().join("outer.AppImage");
        std::fs::write(&image, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o700)).unwrap();
        let unit = root.path().join("blent.service");
        let marker = if mode == "other" {
            String::new()
        } else {
            appimage::marker(&image)
        };
        std::fs::write(&unit, marker).unwrap();
        let systemctl = root.path().join("systemctl");
        std::fs::write(&systemctl, r#"#!/bin/sh
case "$*" in
    *FragmentPath*) printf '%s\n' "$BLENT_T308_UNIT" ;;
    *LoadState*) case "$BLENT_T308_AUTOSTART" in direct|unreachable) exit 1 ;; *) echo loaded ;; esac ;;
    *is-enabled*) if [ "$BLENT_T308_AUTOSTART" = direct ]; then exit 1; fi; echo enabled ;;
    *enable*) if [ "$BLENT_T308_AUTOSTART" = denied ]; then echo denied >&2; exit 1; fi ;;
esac
"#).unwrap();
        std::fs::set_permissions(&systemctl, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "t308_autostart_child", "--nocapture"])
            .env("PATH", root.path())
            .env("HOME", root.path())
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .env("BLENT_T308_UNIT", &unit)
            .env("BLENT_T308_AUTOSTART", mode)
            .env_remove(appimage::LAUNCHER);
        if mode != "unreachable" {
            command.env(appimage::LAUNCHER, image);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
