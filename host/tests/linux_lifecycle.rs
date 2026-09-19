//! T497: real daemon lifecycle in a private PID namespace, with no device access.
#![cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TEST: &str = "t497_idle_daemon_starts_and_reaps_without_attaching_a_display";

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fixture {
    root: tempfile::TempDir,
    binary: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let binary = bin.join("uscreen");
        std::fs::copy(env!("CARGO_BIN_EXE_uscreen"), &binary).unwrap();
        let fixture = Self { root, binary };
        fixture.script("adb", "printf 'List of devices attached\\n\\n'\n");
        fixture.script(
            "evdi_helper",
            "printf 'unexpected helper invocation\\n' >> \"$HOME/helper-called\"; exit 83\n",
        );
        for name in [
            "systemctl",
            "busctl",
            "qdbus",
            "qdbus6",
            "qdbus-qt5",
            "qdbus-qt6",
        ] {
            fixture.script(name, "exit 1\n");
        }
        fixture
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.root.path().join("bin").join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .current_dir(self.root.path())
            .env("HOME", self.root.path())
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_RUNTIME_DIR", self.root.path())
            .env("XDG_CACHE_HOME", self.root.path().join("cache"))
            .env("PATH", self.root.path().join("bin"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/missing-bus", self.root.path().display()),
            )
            .env("XDG_CURRENT_DESKTOP", "T497 isolated fixture")
            .env("RUST_LOG", "uscreen=info")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("USCREEN_FAKE_TABLET");
        command
    }

    fn configuration(&self, video: u16, input: u16) {
        let directory = self.root.path().join("config/uscreen");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("config.toml"), format!(
            "check_updates = false\ninput_touch = false\ninput_pen = false\ninput_pointer = false\nvideo_port = {video}\ninput_port = {input}\nencoder = 'libx264'\n"
        )).unwrap();
    }
}

fn wait_running(child: &mut Child, log: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read_to_string(log).unwrap();
        assert!(
            child.try_wait().unwrap().is_none(),
            "T497: startup exited: {text}"
        );
        if text.contains("uscreen daemon running (PID:") {
            return;
        }
        assert!(Instant::now() < deadline, "T497: startup timed out: {text}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn stop_owned(child: &mut Child) {
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "T497: daemon did not finish normal shutdown: {status}"
            );
            return;
        }
        assert!(Instant::now() < deadline, "T497: daemon failed to retire");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn lifecycle(cli_stop: bool) {
    let fixture = Fixture::new();
    let video = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let input = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    fixture.configuration(
        video.local_addr().unwrap().port(),
        input.local_addr().unwrap().port(),
    );
    drop(video);
    drop(input);
    let log = fixture.root.path().join("daemon.log");
    let output = std::fs::File::create(&log).unwrap();
    let mut child = OwnedChild(
        fixture
            .command()
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap(),
    );
    wait_running(&mut child.0, &log);
    let status = fixture.command().arg("status").output().unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains(&child.0.id().to_string()));
    let duplicate = fixture.command().output().unwrap();
    assert!(
        !duplicate.status.success(),
        "T497: a second daemon claimed the same namespace"
    );
    if cli_stop {
        let stopped = fixture.command().arg("stop").output().unwrap();
        assert!(
            stopped.status.success(),
            "{}",
            String::from_utf8_lossy(&stopped.stderr)
        );
        assert!(child.0.wait().unwrap().success());
        assert!(fixture.command().arg("stop").status().unwrap().success());
    } else {
        stop_owned(&mut child.0);
    }
    assert!(
        !fixture.root.path().join("helper-called").exists(),
        "T497: idle daemon tried to attach EVDI"
    );
    let status = fixture.command().arg("status").output().unwrap();
    assert!(String::from_utf8_lossy(&status.stdout).contains("not running"));
    let text = std::fs::read_to_string(log).unwrap();
    assert!(
        text.contains("uscreen daemon stopped"),
        "T497: missing shutdown confirmation: {text}"
    );
}

#[test]
fn t497_idle_daemon_starts_and_reaps_without_attaching_a_display() {
    if std::env::var_os("USCREEN_TEST_PRIVATE_PID_NAMESPACE").is_some() {
        lifecycle(false);
        lifecycle(true);
        return;
    }
    run_isolated(TEST);
}

fn run_isolated(test: &str) {
    let output = Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--pid",
            "--fork",
            "--mount-proc",
        ])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("USCREEN_TEST_PRIVATE_PID_NAMESPACE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "T497 requires a private PID namespace: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t497_diagnostics_and_display_inventory_are_read_only_without_devices() {
    if std::env::var_os("USCREEN_TEST_PRIVATE_PID_NAMESPACE").is_none() {
        run_isolated("t497_diagnostics_and_display_inventory_are_read_only_without_devices");
        return;
    }
    let fixture = Fixture::new();
    fixture.configuration(8890, 8891);
    fixture.script(
        "evdi_helper",
        "[ \"$#\" = 0 ] || exit 83; echo Usage: >&2; exit 1",
    );
    fixture.script("ffmpeg", "printf ' V..... libx264 fixture encoder\\n'");
    fixture.script("kscreen-doctor", "printf '{\"outputs\":[]}\\n'");
    fixture.script("wpctl", "echo 'T497 isolated PipeWire inventory'");
    let doctor = fixture.command().arg("doctor").output().unwrap();
    assert!(
        doctor.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let text = String::from_utf8_lossy(&doctor.stdout);
    for expected in [
        "=== uscreen doctor ===",
        "update check disabled",
        "no active tablet session",
        "Configuration",
    ] {
        assert!(text.contains(expected), "T497 missing {expected}: {text}");
    }
    let displays = fixture.command().arg("list-displays").output().unwrap();
    assert!(displays.status.success());
    let text = String::from_utf8_lossy(&displays.stdout);
    assert!(text.contains("{\"outputs\":[]}"));
    assert!(text.contains("T497 isolated PipeWire inventory"));
    assert!(!fixture.root.path().join("helper-called").exists());
}
