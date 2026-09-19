#![cfg(target_os = "linux")]
//! Runs the actual GUI binary; workspace tests preserve Cargo feature unification.
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
    std::fs::write(root.join("uscreen/config.toml"), "check_updates = false\n").unwrap();
    let output = std::process::Command::new("timeout")
        .args(["12", "xvfb-run", "-a", "dbus-run-session", "--", "sh", "-c",
            r#"gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled '<true>' || exit 1
            "$1" &
            gui_pid=$!
            trap 'kill "$gui_pid" 2>/dev/null || true; wait "$gui_pid" 2>/dev/null || true' EXIT
            sleep 3
            kill -0 "$gui_pid" || exit 1
            xwininfo -root -tree"#, "sh"])
        .arg(env!("CARGO_BIN_EXE_uscreen-gui"))
        .env("XDG_CONFIG_HOME", &root)
        .env("WINIT_UNIX_BACKEND", "x11")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("GSETTINGS_BACKEND", "memory")
        .env_remove("WAYLAND_DISPLAY")
        .output().expect("GUI smoke test requires Xvfb, xauth, dbus-run-session and xwininfo");
    let _ = std::fs::remove_dir_all(root);
    let stderr = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.status.success(), "GUI startup failed: {stderr}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("UScreen"),
        "GUI window missing: {stderr}"
    );
    assert!(
        !stderr.contains("panicked") && !stderr.contains("there is no reactor running"),
        "worker failed: {stderr}"
    );
}
