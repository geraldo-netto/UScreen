//! Install/release regressions: all external writes and builds are sandboxed.
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

struct Sandbox(PathBuf);
impl Sandbox {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("uscreen-tooling-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }
    fn script(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.write(name, contents);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    fn path(&self) -> std::ffi::OsString {
        std::env::join_paths(
            std::iter::once(self.0.join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

#[test]
fn t034_ci_propagates_helper_build_failure_and_ships_helper() {
    let yaml = std::fs::read_to_string(repo().join(".github/workflows/build.yml")).unwrap();
    let step = yaml
        .split("- name: Build EVDI helper")
        .nth(1)
        .unwrap()
        .lines()
        .find_map(|l| l.trim().strip_prefix("run: "))
        .unwrap();
    let sandbox = Sandbox::new("ci");
    sandbox.script("bin/make", "#!/bin/sh\nexit 42\n");
    let output = Command::new("sh")
        .args(["-c", step])
        .env("PATH", sandbox.path())
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "CI swallowed helper compilation failure"
    );
    let artifacts = yaml
        .split("name: uscreen-linux-x86_64")
        .nth(1)
        .unwrap()
        .split("build-android:")
        .next()
        .unwrap();
    assert!(artifacts
        .lines()
        .any(|l| l.trim() == "host/evdi/evdi_helper"));
}

#[test]
fn t035_setup_installs_udev_rule_and_two_boot_devices() {
    let sandbox = Sandbox::new("setup");
    sandbox.script(
        "bin/sudo",
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$USCREEN_TEST_LOG"
if [ "$1" = tee ]; then cat >> "$USCREEN_TEST_LOG"; fi
"#,
    );
    let log = sandbox.0.join("commands");
    let output = Command::new("make")
        .arg("setup-system")
        .current_dir(repo())
        .env("PATH", sandbox.path())
        .env("USCREEN_TEST_LOG", &log)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commands = std::fs::read_to_string(log).unwrap();
    assert!(commands.contains("initial_device_count=2"), "{commands}");
    assert!(commands.contains("install -Dm644 packaging/60-uscreen-uinput.rules /etc/udev/rules.d/60-uscreen-uinput.rules"), "{commands}");
    assert!(commands.contains("udevadm control --reload"));
    assert!(commands.contains("udevadm trigger --name-match=uinput"));
}

#[test]
fn t073_install_does_not_require_unused_edid_or_python() {
    let output = Command::new("make")
        .args(["-n", "install"])
        .current_dir(repo())
        .output()
        .unwrap();
    assert!(output.status.success());
    let recipe = String::from_utf8(output.stdout).unwrap();
    assert!(
        !recipe.contains("gen-edid.py") && !recipe.contains("s9ultra.bin"),
        "{recipe}"
    );
}

fn rejected_publish(name: &str, notes: bool, expected: &str) {
    let sandbox = Sandbox::new(name);
    sandbox.write("Makefile", "VERSION = 1.2.3\n");
    for f in ["host/Cargo.toml", "gui/Cargo.toml", "common/Cargo.toml"] {
        sandbox.write(f, "version = \"1.2.3\"\n");
    }
    sandbox.write("android/app/build.gradle.kts", "versionName = \"1.2.3\"\n");
    sandbox.write("packaging/arch/PKGBUILD", "pkgver=1.2.3\n");
    sandbox.write("CHANGELOG.md", "## 1.2.3 — 2026-09-16\n");
    sandbox.write("notes.md", "Release notes\n");
    sandbox.script("scripts/update-release-metadata.sh", "#!/bin/sh\nexit 0\n");
    sandbox.script(
        "scripts/build-release.sh",
        "#!/bin/sh\ntouch build-called\nexit 42\n",
    );
    sandbox.script(
        "packaging/build-packages.sh",
        "#!/bin/sh\ntouch build-called\nexit 42\n",
    );
    sandbox.script("bin/git", "#!/bin/sh\n[ \"$1\" != rev-parse ]\n");
    let script = std::fs::read_to_string(repo().join("scripts/publish-release.sh")).unwrap();
    let script = sandbox.script("scripts/publish-release.sh", &script);
    let mut cmd = Command::new(script);
    cmd.current_dir(&sandbox.0)
        .env("PATH", sandbox.path())
        .env("GH_TOKEN", "test-placeholder")
        .env("RELEASE_DATE", "2026-09-16");
    if notes {
        cmd.arg("notes.md");
    }
    let output = cmd.output().unwrap();
    assert!(!output.status.success());
    assert!(
        !sandbox.0.join("build-called").exists(),
        "build started before {expected} preflight"
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains(expected), "{text}");
}

#[test]
fn t075_missing_notes_fail_before_build() {
    rejected_publish("notes", false, "usage:");
}
#[test]
fn t075_missing_tag_fails_before_build() {
    rejected_publish("tag", true, "tag v1.2.3 does not exist");
}

#[test]
fn t103_fake_tablet_handles_partial_tcp_io() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_fake_tablet.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t095_source_installs_rebuild_all_inputs_but_release_uses_shipped_files() {
    let sandbox = Sandbox::new("source-rebuild");
    let source = std::fs::read_to_string(repo().join("scripts/install.sh")).unwrap();
    let source = source.strip_suffix("main \"$@\"\n").unwrap();
    let script = sandbox.script(
        "scripts/install.sh",
        &format!("{source}\nbuild_if_needed\n"),
    );
    sandbox.script(
        "bin/make",
        "#!/bin/sh\nprintf 'build\\n' >> \"$USCREEN_TEST_LOG\"\n",
    );
    let log = sandbox.0.join("builds");
    // A lone stale daemon, complete but outdated binaries, then a release bundle.
    sandbox.write("target/release/uscreen", "old");
    for expected in [1, 2, 2] {
        let output = Command::new(&script)
            .env("PATH", sandbox.path())
            .env("USCREEN_TEST_LOG", &log)
            .output()
            .unwrap();
        assert!(output.status.success());
        let count = std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .count();
        assert_eq!(
            count, expected,
            "source install skipped incremental rebuild"
        );
        sandbox.write("target/release/uscreen-gui", "old");
        sandbox.write("host/evdi/evdi_helper", "old");
        sandbox.write("host/src/main.rs", "new source");
        if expected == 2 {
            sandbox.write("bin/uscreen", "release");
        }
    }
}

#[test]
fn t096_desktop_launches_installed_gui_with_stale_path() {
    let sandbox = Sandbox::new("desktop-install");
    let makefile = std::fs::read_to_string(repo().join("Makefile"))
        .unwrap()
        .replace("${HOME}", sandbox.0.to_str().unwrap());
    sandbox.write("Makefile", &makefile);
    for name in ["uscreen", "uscreen-gui"] {
        sandbox.script(
            &format!("target/release/{name}"),
            "#!/bin/sh\necho installed\n",
        );
    }
    sandbox.script("host/evdi/evdi_helper", "#!/bin/sh\nexit 0\n");
    sandbox.write(
        "scripts/uscreen.desktop",
        &std::fs::read_to_string(repo().join("scripts/uscreen.desktop")).unwrap(),
    );
    sandbox.script("bin/systemctl", "#!/bin/sh\nexit 0\n");
    sandbox.script("bin/uscreen-gui", "#!/bin/sh\necho stale\n");
    let output = Command::new("make")
        .args(["-o", "build", "install"])
        .current_dir(&sandbox.0)
        .env("PATH", sandbox.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let desktop =
        std::fs::read_to_string(sandbox.0.join(".local/share/applications/uscreen.desktop"))
            .unwrap();
    let exec = desktop
        .lines()
        .find_map(|line| line.strip_prefix("Exec="))
        .unwrap();
    let launched = Command::new("sh")
        .args(["-c", exec])
        .env("PATH", sandbox.path())
        .output()
        .unwrap();
    assert!(launched.status.success());
    assert_eq!(
        String::from_utf8_lossy(&launched.stdout).trim(),
        "installed"
    );
}

#[test]
fn t097_make_setup_creates_missing_configuration_directories() {
    let sandbox = Sandbox::new("minimal-setup");
    let makefile = std::fs::read_to_string(repo().join("Makefile"))
        .unwrap()
        .replace("/etc/", &format!("{}/etc/", sandbox.0.display()))
        .replace("/sys/", &format!("{}/sys/", sandbox.0.display()));
    sandbox.write("Makefile", &makefile);
    sandbox.write("sys/devices/evdi/count", "2");
    sandbox.write(
        "packaging/60-uscreen-uinput.rules",
        &std::fs::read_to_string(repo().join("packaging/60-uscreen-uinput.rules")).unwrap(),
    );
    sandbox.script("bin/sudo", "#!/bin/sh\nexec \"$@\"\n");
    for name in ["modprobe", "udevadm"] {
        sandbox.script(&format!("bin/{name}"), "#!/bin/sh\nexit 0\n");
    }
    let output = Command::new("make")
        .arg("setup-system")
        .current_dir(&sandbox.0)
        .env("PATH", sandbox.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(sandbox.0.join("etc/modprobe.d/uscreen-evdi.conf").is_file());
    assert!(sandbox.0.join("etc/modules-load.d/uscreen.conf").is_file());
    assert!(sandbox
        .0
        .join("etc/udev/rules.d/60-uscreen-uinput.rules")
        .is_file());
}

fn release_tests(pattern: &str) {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_release.py"))
        .args(["-k", pattern])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t100_release_requires_matching_head_local_and_remote_tags() {
    release_tests("t100");
}

#[test]
fn t118_release_credentials_never_appear_in_command_arguments() {
    release_tests("t118");
}

#[test]
fn t117_release_stays_draft_until_all_assets_are_verified() {
    release_tests("t117");
}

#[test]
fn t101_package_failures_cannot_reuse_old_assets() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_packages.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t102_local_distribution_requires_apk_and_bundled_library() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_distribution.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
