#![cfg(target_os = "linux")]
//! Runs the actual GUI binary; workspace tests preserve Cargo feature unification.
use std::os::unix::fs::PermissionsExt;

fn stub(root: &std::path::Path, name: &str, body: &str) {
    let path = root.join("bin").join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn t078_workspace_gui_and_accessibility_start_without_worker_panics() {
    // Cargo may compile integration-test binary prerequisites with only this
    // package's feature set. Explicitly exercise the shipped workspace build.
    let mut build = std::process::Command::new(env!("CARGO"));
    build.args(["build", "--workspace"]).current_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    );
    if !cfg!(debug_assertions) {
        build.arg("--release");
    }
    let built = build.output().unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let root = std::env::temp_dir().join(format!("uscreen-gui-startup-{}", std::process::id()));
    std::fs::create_dir_all(root.join("uscreen")).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir(root.join("bin")).unwrap();
    stub(&root, "curl", "printf checked > \"$XDG_CONFIG_HOME/update-checked\"\necho '{\"tag_name\":\"v999.0.0\",\"draft\":false,\"prerelease\":false}'");
    stub(&root, "adb", "printf 'List of devices attached\\n\\n'");
    stub(&root, "systemctl", "exit 1");
    std::fs::write(root.join("uscreen/config.toml"), "check_updates = true\n").unwrap();
    let path = std::env::join_paths(
        std::iter::once(root.join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let output = std::process::Command::new("timeout")
        .args(["12", "xvfb-run", "-a", "dbus-run-session", "--", "sh", "-c",
            r#"gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled '<true>' || exit 1
            "$1" &
            gui_pid=$!
            trap 'kill "$gui_pid" 2>/dev/null || true; wait "$gui_pid" 2>/dev/null || true' EXIT
            sleep 3
            kill -0 "$gui_pid" || exit 1
            xwininfo -root -tree || exit 1
            python3 "$2" "$gui_pid" || exit 1
            wait "$gui_pid"
            result=$?
            trap - EXIT
            exit "$result""#, "sh"])
        .arg(env!("CARGO_BIN_EXE_uscreen-gui"))
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/close_window.py"))
        .env("XDG_CONFIG_HOME", &root)
        .env("HOME", &root)
        .env("XDG_RUNTIME_DIR", &root)
        .env("PATH", path)
        .env("WINIT_UNIX_BACKEND", "x11")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("GSETTINGS_BACKEND", "memory")
        .env_remove("WAYLAND_DISPLAY")
        .output().expect("GUI smoke test requires Xvfb, xauth, dbus-run-session and xwininfo");
    let checked_update = root.join("update-checked").exists();
    let _ = std::fs::remove_dir_all(root);
    let stderr = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.status.success(), "GUI startup failed: {stderr}");
    assert!(
        checked_update,
        "T497: startup did not complete its isolated update check"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("UScreen"),
        "GUI window missing: {stderr}"
    );
    assert!(
        !stderr.contains("panicked") && !stderr.contains("there is no reactor running"),
        "worker failed: {stderr}"
    );
}
