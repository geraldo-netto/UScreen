//! Install/release regressions: all external writes and builds are sandboxed.
#![cfg(target_os = "linux")]

#[test]
fn t500_rust_formatter_checks_included_supervisor_modules() {
    let output = std::process::Command::new("python3")
        .arg(repo().join("scripts/tests/test_format_rust.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

struct Sandbox(PathBuf);
impl Sandbox {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("blent-tooling-{}-{name}", std::process::id()));
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
fn t585_blent_identities_preserve_original_notices() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_identity.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t382_benchmark_units_and_window_statistics() {
    let output = Command::new("python3")
        .args(["-m", "unittest", "discover", "-s"])
        .arg(repo().join("scripts/tests"))
        .args(["-p", "test_benchmark*.py"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t483_codec_validation_and_cache_regressions_run_in_normal_suite() {
    let output = Command::new("python3")
        .args(["-m", "unittest", "discover", "-s"])
        .arg(repo().join("scripts/tests"))
        .args(["-p", "test_codec*.py"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t342_build_outputs_are_ignored_but_sources_are_visible() {
    let sandbox = Sandbox::new("build-ignores");
    sandbox.write(
        ".gitignore",
        &std::fs::read_to_string(repo().join(".gitignore")).unwrap(),
    );
    assert!(Command::new("git")
        .arg("init")
        .arg(&sandbox.0)
        .output()
        .unwrap()
        .status
        .success());
    for tree in ["target", "target-deb12", "target-portability"] {
        let path = sandbox.write(&format!("{tree}/release/blent"), "generated output");
        assert!(
            Command::new("git")
                .args(["-c", "core.excludesFile=/dev/null"])
                .args(["check-ignore", "-q"])
                .arg(path)
                .current_dir(&sandbox.0)
                .status()
                .unwrap()
                .success(),
            "build output in {tree} is visible to Git"
        );
    }
    let source = sandbox.write("host/src/main.rs", "project source");
    assert_eq!(
        Command::new("git")
            .args(["-c", "core.excludesFile=/dev/null"])
            .args(["check-ignore", "-q"])
            .arg(source)
            .current_dir(&sandbox.0)
            .status()
            .unwrap()
            .code(),
        Some(1)
    );
}

#[test]
fn t489_private_signing_and_local_outputs_stay_out_of_git() {
    let sandbox = Sandbox::new("signing-ignores");
    sandbox.write(
        ".gitignore",
        &std::fs::read_to_string(repo().join(".gitignore")).unwrap(),
    );
    assert!(Command::new("git")
        .arg("init")
        .arg(&sandbox.0)
        .output()
        .unwrap()
        .status
        .success());
    let cases = [
        ("android/release.p12", true),
        ("android/release.P12", true),
        ("signing/release.pfx", true),
        ("signing/release.PFX", true),
        ("signing/release.key", true),
        ("signing/private.pem", true),
        ("signing/id_ed25519", true),
        ("signing/id_rsa", true),
        ("signing/keystore-password.txt", true),
        ("nested/keystore.properties", true),
        ("nested/.env.secret", true),
        (".venv/bin/python", true),
        ("scripts/.pytest_cache/v/cache/nodeids", true),
        ("scripts/tests/__pycache__/fixture.pyo", true),
        ("android/.kotlin/session", true),
        ("android/app/release/app-release.aab", true),
        ("Blent.AppImage", true),
        ("Blent.AppDir/AppRun", true),
        ("host/evdi/conversion.o", true),
        ("host/evdi/libconversion.a", true),
        ("hs_err_pid1234.log", true),
        ("docs/release-certificate.pem", false),
        ("Cargo.lock", false),
        ("android/gradle/wrapper/gradle-wrapper.jar", false),
        (".cargo/config.toml", false),
        ("host/src/encoder.rs", false),
        ("docs/benchmarks/trial/red.log", false),
        ("docs/benchmarks/fixtures.tar.gz", false),
        ("docs/benchmarks/trial/motion.bin.gz", false),
        ("scripts/tests/example.pub", false),
        ("testdata/raw-frame.bin", false),
        ("signing/keystore.properties.example", false),
    ];
    for (name, ignored) in cases {
        let path = sandbox.write(name, "non-secret regression fixture");
        let status = Command::new("git")
            .args(["-c", "core.excludesFile=/dev/null", "check-ignore", "-q"])
            .arg(path)
            .current_dir(&sandbox.0)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(i32::from(!ignored)), "T489: {name}");
    }
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
        .split("name: blent-linux-x86_64")
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
printf '%s\n' "$*" >> "$BLENT_TEST_LOG"
if [ "$1" = tee ]; then cat >> "$BLENT_TEST_LOG"; fi
"#,
    );
    let log = sandbox.0.join("commands");
    let output = Command::new("make")
        .arg("setup-system")
        .current_dir(repo())
        .env("PATH", sandbox.path())
        .env("BLENT_TEST_LOG", &log)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commands = std::fs::read_to_string(log).unwrap();
    assert!(commands.contains("initial_device_count=2"), "{commands}");
    assert!(commands.contains("install -Dm644 packaging/60-blent-uinput.rules /etc/udev/rules.d/60-blent-uinput.rules"), "{commands}");
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
fn t321_edid_tool_rejects_invalid_and_low_clock_modes() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_gen_edid.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t103_t242_fake_tablet_handles_partial_io_and_runtime_paths() {
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
        "#!/bin/sh\nprintf 'build\\n' >> \"$BLENT_TEST_LOG\"\n",
    );
    let log = sandbox.0.join("builds");
    // A lone stale daemon, complete but outdated binaries, then a release bundle.
    sandbox.write("target/release/blent", "old");
    for expected in [1, 2, 2] {
        let output = Command::new(&script)
            .env("PATH", sandbox.path())
            .env("BLENT_TEST_LOG", &log)
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
        sandbox.write("target/release/blent-gui", "old");
        sandbox.write("host/evdi/evdi_helper", "old");
        sandbox.write("host/src/main.rs", "new source");
        if expected == 2 {
            sandbox.write("bin/blent", "release");
        }
    }
}

#[test]
fn t141_installers_launch_desktop_paths_with_reserved_characters() {
    for installer in ["source", "make"] {
        let sandbox = Sandbox::new(&format!("desktop-escaping-{installer}"));
        let home = sandbox
            .0
            .join("home spaces & 'quotes' \\\" $dollar `tick` %percent | semi;");
        sandbox.script("bin/systemctl", "#!/bin/sh\nexit 0\n");
        sandbox.script("bin/gtk-update-icon-cache", "#!/bin/sh\nexit 0\n");
        sandbox.script("bin/kbuildsycoca6", "#!/bin/sh\nexit 0\n");
        for name in ["blent", "blent-gui", "evdi_helper"] {
            sandbox.script(
                &format!("target/release/{name}"),
                "#!/bin/sh\nprintf '%s' \"$0\" > \"$BLENT_TEST_LAUNCHED\"\n",
            );
        }
        sandbox.script("host/evdi/evdi_helper", "#!/bin/sh\nexit 0\n");
        for entry in std::fs::read_dir(repo().join("scripts")).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let name = entry.file_name();
                let to = sandbox.0.join("scripts").join(name);
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::copy(entry.path(), to).unwrap();
            }
        }
        let mut command = if installer == "make" {
            sandbox.write(
                "Makefile",
                &std::fs::read_to_string(repo().join("Makefile")).unwrap(),
            );
            let mut command = Command::new("make");
            command.args(["-o", "build", "install"]);
            command
        } else {
            let source = std::fs::read_to_string(repo().join("scripts/install.sh")).unwrap();
            let source = source.strip_suffix("main \"$@\"\n").unwrap();
            let script =
                sandbox.script("scripts/install.sh", &format!("{source}\ninstall_files\n"));
            Command::new(script)
        };
        let output = command
            .current_dir(&sandbox.0)
            .env("PATH", sandbox.path())
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T141 {installer}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let desktop = home.join(".local/share/applications/blent.desktop");
        let marker = sandbox.0.join("launched");
        let output = Command::new("gio")
            .arg("launch")
            .arg(&desktop)
            .env("BLENT_TEST_LAUNCHED", &marker)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T141 {installer}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(marker).unwrap(),
            home.join(".local/bin/blent-gui").to_str().unwrap()
        );
    }
}

#[test]
fn t096_desktop_launches_installed_gui_with_stale_path() {
    let sandbox = Sandbox::new("desktop-install");
    sandbox.script(
        "scripts/install.sh",
        &std::fs::read_to_string(repo().join("scripts/install.sh")).unwrap(),
    );
    let makefile = std::fs::read_to_string(repo().join("Makefile")).unwrap();
    sandbox.write("Makefile", &makefile);
    for name in ["blent", "blent-gui"] {
        sandbox.script(
            &format!("target/release/{name}"),
            "#!/bin/sh\necho installed\n",
        );
    }
    sandbox.script("host/evdi/evdi_helper", "#!/bin/sh\nexit 0\n");
    sandbox.write(
        "scripts/blent.desktop",
        &std::fs::read_to_string(repo().join("scripts/blent.desktop")).unwrap(),
    );
    sandbox.write(
        "scripts/blent.service",
        &std::fs::read_to_string(repo().join("scripts/blent.service")).unwrap(),
    );
    sandbox.write(
        "scripts/blent-service-autostart.desktop",
        &std::fs::read_to_string(repo().join("scripts/blent-service-autostart.desktop")).unwrap(),
    );
    sandbox.script(
        "scripts/write-desktop-entry.sh",
        &std::fs::read_to_string(repo().join("scripts/write-desktop-entry.sh")).unwrap(),
    );
    sandbox.script(
        "scripts/write-systemd-service.sh",
        &std::fs::read_to_string(repo().join("scripts/write-systemd-service.sh")).unwrap(),
    );
    sandbox.script("bin/systemctl", "#!/bin/sh\nexit 0\n");
    sandbox.script("bin/blent-gui", "#!/bin/sh\necho stale\n");
    let output = Command::new("make")
        .args(["-o", "build", "install"])
        .current_dir(&sandbox.0)
        .env("PATH", sandbox.path())
        .env("HOME", &sandbox.0)
        .env("XDG_DATA_HOME", sandbox.0.join(".local/share"))
        .env("XDG_CONFIG_HOME", sandbox.0.join(".config"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let desktop =
        std::fs::read_to_string(sandbox.0.join(".local/share/applications/blent.desktop")).unwrap();
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
    sandbox.write(
        "scripts/setup-evdi.sh",
        &std::fs::read_to_string(repo().join("scripts/setup-evdi.sh"))
            .unwrap()
            .replace("/sys/", &format!("{}/sys/", sandbox.0.display())),
    );
    sandbox.write("sys/devices/evdi/count", "2");
    sandbox.write(
        "packaging/60-blent-uinput.rules",
        &std::fs::read_to_string(repo().join("packaging/60-blent-uinput.rules")).unwrap(),
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
    assert!(sandbox.0.join("etc/modprobe.d/blent-evdi.conf").is_file());
    assert!(sandbox.0.join("etc/modules-load.d/blent.conf").is_file());
    assert!(sandbox
        .0
        .join("etc/udev/rules.d/60-blent-uinput.rules")
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
fn t277_release_metadata_validates_before_writing() {
    release_tests("t277");
}

#[test]
fn t350_release_metadata_prepares_unpublished_fork() {
    release_tests("t350");
}

#[test]
fn t100_release_requires_matching_head_local_and_remote_tags() {
    release_tests("t100");
}

#[test]
fn t263_release_rejects_source_changes_during_build() {
    release_tests("t263");
}

#[test]
fn t262_release_metadata_requires_literal_equality() {
    release_tests("t262");
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
fn t225_project_links_and_release_requests_target_fork() {
    release_tests("t225");
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

#[test]
fn t302_distribution_regressions_survive_release_version_bumps() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_version_fixtures.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t129_distributed_notices_and_readme_links_exist() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_notices.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t269_setup_preserves_live_devices_and_reports_missing_capacity() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_evdi_setup.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t214_installer_preserves_dependency_and_setup_commands() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_installer.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t220_make_shares_jobserver_without_executing_dry_runs() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_make.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t336_source_workflows_consume_their_selected_build_outputs() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_build_output.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t233_user_installers_share_xdg_paths() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_user_paths.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t250_official_android_release_checks_fork_certificate() {
    release_tests("t250");
    let status = Command::new("python3")
        .arg(repo().join("scripts/tests/test_release_apk.py"))
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn t308_appimage_packaging_launch_and_invalid_inputs() {
    let output = Command::new("python3")
        .arg(repo().join("scripts/tests/test_appimage.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    release_tests("t308");
}
