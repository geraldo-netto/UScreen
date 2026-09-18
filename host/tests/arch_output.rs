//! T467: source-build the production PKGBUILD with real Cargo and isolated assets.
use std::{path::Path, process::Command};

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn executable(root: &Path, name: &str, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(root, name, text);
    std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn prepare(root: &Path, version: &str) -> std::path::PathBuf {
    let source = root.join(format!("UScreen-{version}"));
    write(
        &source,
        "Cargo.toml",
        "[workspace]\nmembers = [\"host\", \"gui\"]\nresolver = \"2\"\n",
    );
    for (folder, name) in [("host", "uscreen"), ("gui", "uscreen-gui")] {
        write(
            &source,
            &format!("{folder}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        );
        write(
            &source,
            &format!("{folder}/src/main.rs"),
            &format!("fn main() {{ println!(\"fresh-{name}\"); }}\n"),
        );
        executable(
            &source,
            &format!("target/release/{name}"),
            "#!/bin/sh\necho stale\n",
        );
    }
    for name in [
        "scripts/setup-evdi.sh",
        "host/evdi/evdi_helper",
        "scripts/uscreen.desktop",
        "packaging/icons/uscreen.svg",
        "packaging/icons/uscreen-pen.svg",
        "packaging/uscreen.service",
        "packaging/uscreen-evdi.conf",
        "packaging/uscreen-modules.conf",
        "packaging/60-uscreen-uinput.rules",
        "LICENSE",
        "licenses/libevdi-LGPL-2.1.txt",
    ] {
        write(&source, name, "fixture");
    }
    executable(
        &source,
        "scripts/copy-distribution-docs.sh",
        "#!/bin/sh\nmkdir -p -- \"$1\"\n",
    );
    write(root, "evdi-1.15.0/library/libevdi.so.1.15.0", "fixture");
    executable(root, "bin/make", "#!/bin/sh\nexit 0\n");
    executable(root, "bin/gcc", "#!/bin/sh\nexit 0\n");
    let output = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    source
}

fn builds_fresh_package(config_override: bool) {
    let root = tempfile::tempdir().unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let pkgbuild = repo.join("packaging/arch/PKGBUILD");
    let text = std::fs::read_to_string(&pkgbuild).unwrap();
    let version = text
        .lines()
        .find_map(|line| line.strip_prefix("pkgver="))
        .unwrap();
    let source = prepare(root.path(), version);
    let external = root.path().join("external-target");
    if config_override {
        write(
            &source,
            ".cargo/config.toml",
            &format!("[build]\ntarget-dir = {:?}\n", external),
        );
    }
    let path = std::env::join_paths(
        std::iter::once(root.path().join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let package = root.path().join("package");
    let mut build = Command::new("bash");
    build
        .args([
            "-c",
            "set -eu\nsource \"$USCREEN_PKGBUILD\"\nbuild\ncd \"$srcdir\"\npackage\n",
        ])
        .env("USCREEN_PKGBUILD", pkgbuild)
        .env("srcdir", root.path())
        .env("pkgdir", &package)
        .env("PATH", path)
        .current_dir(root.path());
    if config_override {
        build.env_remove("CARGO_TARGET_DIR");
    } else {
        build.env("CARGO_TARGET_DIR", external);
    }
    let output = build.output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for name in ["uscreen", "uscreen-gui"] {
        let output = Command::new(package.join("usr/bin").join(name))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("fresh-{name}\n"),
            "T467: packaged stale {name}"
        );
    }
}

#[test]
fn t467_arch_source_build_pins_output_despite_environment_override() {
    builds_fresh_package(false);
}

#[test]
fn t467_arch_source_build_pins_output_despite_cargo_config_override() {
    builds_fresh_package(true);
}
