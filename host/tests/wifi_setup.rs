//! T514: Wi-Fi setup requires successful ADB observations before saving a route.
#![cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

const ADB: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/adb.calls"
case "$*" in
  devices|'devices -l') printf 'List of devices attached\nTABLET\tdevice\n';;
  *'shell pm path '*) echo 'package:/fixture.apk';;
  *'tcpip 5555') [ "$USCREEN_T514_MODE" != tcpip-failed ];;
  *'addr show wlan0') echo 'inet 192.0.2.10/24'; [ "$USCREEN_T514_MODE" = success ];;
  *'route get 1.1.1.1') echo 'src 192.0.2.20'; [ "$USCREEN_T514_MODE" != probe-failed ];;
  *'inet addr') echo 'inet 192.0.2.30/24'; exit 1;;
  'connect '*) echo "connected to $2"; [ "$USCREEN_T514_MODE" != connect-failed ];;
  'disconnect '*) exit 0;;
  *) exit 85;;
esac
"#;

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let tool = bin.join("adb");
        std::fs::write(&tool, ADB).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        let fixture = Self(root);
        std::fs::create_dir_all(fixture.config().parent().unwrap()).unwrap();
        std::fs::write(fixture.config(), "wifi_address = '192.0.2.99:5555'\n").unwrap();
        fixture
    }
    fn config(&self) -> PathBuf {
        self.0.path().join("config/uscreen/config.toml")
    }
    fn run(&self, mode: &str, off: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_uscreen"));
        command
            .arg("wifi")
            .env("USCREEN_T514_MODE", mode)
            .env("HOME", self.0.path())
            .env("XDG_RUNTIME_DIR", self.0.path())
            .env("XDG_CONFIG_HOME", self.0.path().join("config"))
            .env("PATH", self.0.path().join("bin"))
            .env_remove("USCREEN_FAKE_TABLET");
        if off {
            command.arg("--off");
        }
        command.output().unwrap()
    }
    fn address(&self) -> String {
        uscreen_config::FileConfig::load_at(&self.config()).wifi_address
    }
}

#[test]
fn t514_failed_connect_does_not_persist_a_new_address() {
    let fixture = Fixture::new();
    let result = fixture.run("connect-failed", false);
    assert!(
        !result.status.success(),
        "T514 accepted failed connect: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(fixture.address(), "192.0.2.99:5555");
}

#[test]
fn t514_failed_address_probes_do_not_admit_their_stdout() {
    let fixture = Fixture::new();
    let result = fixture.run("probe-failed", false);
    assert!(
        !result.status.success(),
        "T514 accepted failed address probe: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(fixture.address(), "192.0.2.99:5555");
    let calls = std::fs::read_to_string(fixture.0.path().join("adb.calls")).unwrap();
    assert!(!calls.lines().any(|line| line.starts_with("connect ")));
}

#[test]
fn t514_successful_address_fallback_and_off_preserve_the_cli_contract() {
    for (mode, expected) in [
        ("success", "192.0.2.10:5555"),
        ("fallback", "192.0.2.20:5555"),
    ] {
        let fixture = Fixture::new();
        let result = fixture.run(mode, false);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fixture.address(), expected);
        assert!(fixture.run(mode, true).status.success());
        assert_eq!(fixture.address(), "");
        let calls = std::fs::read_to_string(fixture.0.path().join("adb.calls")).unwrap();
        assert!(calls
            .lines()
            .any(|line| line == format!("disconnect {expected}")));
    }
    let fixture = Fixture::new();
    assert!(!fixture.run("tcpip-failed", false).status.success());
    assert_eq!(fixture.address(), "192.0.2.99:5555");
}
