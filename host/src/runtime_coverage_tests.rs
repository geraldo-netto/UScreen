//! T497: host recovery paths run in a child with private runtime/config directories.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

const TEST: &str = "runtime_coverage_tests::t497_private_runtime_failures_and_transport_recovery";

fn isolated_command() -> std::process::Command {
    let executable = std::env::current_exe().unwrap();
    if std::env::var_os("BLENT_TEST_PRIVATE_PID_NAMESPACE").is_some()
        && Path::new("/.dockerenv").is_file()
    {
        return std::process::Command::new(executable);
    }
    let mut command = std::process::Command::new("unshare");
    command.args(["--user", "--map-root-user", "--mount", "--propagation", "private",
        "--pid", "--fork", "--mount-proc", "/bin/sh", "-c",
        "set -e; mount -t tmpfs -o mode=700,nosuid,nodev tmpfs /run/user; PATH=\"$HOME\" exec \"$@\"",
        "blent-t497-runtime"]);
    command.arg(executable);
    command
}

fn isolated() {
    let root = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let output = isolated_command()
        .args(["--exact", TEST, "--nocapture"])
        .env("BLENT_T497_HOST_RUNTIME", "1")
        .env("BLENT_FAKE_TABLET", "one, two,,one, existing, ")
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_RUNTIME_DIR", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn runtime_contract(root: &Path) {
    assert!(create_session_token(true).unwrap().is_some());
    assert!(create_session_token(false).unwrap().is_none());
    assert!(session_ledger().is_some());
    // An absent/non-directory XDG path deliberately falls back to /run/user.
    // Keep an existing private fixture directory so validation cannot escape it.
    let invalid = root.join("unsafe-runtime");
    std::fs::create_dir(&invalid).unwrap();
    std::fs::set_permissions(&invalid, std::fs::Permissions::from_mode(0o777)).unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", &invalid);
    let error = match create_session_token(true) {
        Ok(_) => panic!("T497 unsafe runtime was accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("require_token is on"));
    assert!(session_ledger().is_none());
    let absent = root.join("not-a-runtime-directory");
    std::fs::write(&absent, "fixture").unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", &absent);
    assert!(
        runtime::runtime_dir().unwrap().starts_with(root),
        "T497 runtime fallback escaped fixture"
    );
    assert!(create_session_token(true).unwrap().is_some());
    std::env::set_var("XDG_RUNTIME_DIR", root);
    let mut devices = vec!["existing".into()];
    add_fake_tablets(&mut devices);
    assert_eq!(devices, ["existing", "one", "two"]);
    let identities = std::collections::HashMap::new();
    assert_eq!(
        current_transport(&devices, Some("two"), &identities).as_deref(),
        Some("two")
    );
    assert!(current_transport(&devices, Some("absent"), &identities).is_none());
    assert!(current_transport(&devices, None, &identities).is_none());
}

fn config_contract() {
    let path = config::config_path().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "fps = 0\n").unwrap();
    let settings = config::FileConfig::load();
    heal_config(&settings);
    let raw: config::FileConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw.fps, settings.fps);
    assert!(crate::test_logging::text().contains("Rewrote out-of-range settings"));
    std::fs::write(&path, "fps = 0\n").unwrap();
    let lock = path.with_extension("lock");
    std::fs::remove_file(&lock).unwrap();
    std::fs::create_dir(&lock).unwrap();
    heal_config(&settings);
    std::fs::remove_dir(&lock).unwrap();
    assert!(crate::test_logging::text().contains("Could not rewrite the config file"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "fps = 0\n");
}

async fn relaunch_contract() {
    let requests: Vec<_> = (0..128)
        .map(|index| (index.to_string(), Arc::new(tokio::sync::Notify::new())))
        .collect();
    requests[127].1.notify_one();
    assert_eq!(wait_extra_relaunch(requests).await, "127");
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(1),
        wait_extra_relaunch(vec![])
    )
    .await
    .is_err());
}

async fn wifi_contract(root: &Path) {
    let path = config::config_path().unwrap();
    std::fs::write(&path, "wifi_address = '192.0.2.9:5555'\n").unwrap();
    let adb = root.join("adb");
    std::fs::write(
        &adb,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/adb-calls"
if [ "$1" = connect ]; then
  printf "wifi_address = '192.0.2.10:5555'\n" > "$XDG_CONFIG_HOME/blent/config.toml"
  printf 'connected\n'
fi
"#,
    )
    .unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        WifiReconnect::new(path.clone(), adb.to_string_lossy().into_owned())
            .connect()
            .await
            .is_none()
    );
    assert_eq!(
        config::FileConfig::load_at(&path).wifi_address,
        "192.0.2.10:5555"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("adb-calls")).unwrap(),
        "connect 192.0.2.9:5555\ndisconnect 192.0.2.9:5555\n"
    );
}

#[tokio::test]
async fn t497_private_runtime_failures_and_transport_recovery() {
    if std::env::var_os("BLENT_T497_HOST_RUNTIME").is_none() {
        isolated();
        return;
    }
    crate::test_logging::enable();
    let root = PathBuf::from(std::env::var_os("HOME").unwrap());
    runtime_contract(&root);
    config_contract();
    relaunch_contract().await;
    wifi_contract(&root).await;
}

#[tokio::test(start_paused = true)]
async fn t497_stop_timeout_leaves_a_nonresponsive_owned_daemon_reported() {
    use tokio::io::AsyncBufReadExt;
    let mut child = tokio::process::Command::new("/bin/sh")
        .args([
            "-c",
            "trap '' TERM; echo blent > /proc/$$/comm; echo ready; read value",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut ready = String::new();
    tokio::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .await
        .unwrap();
    assert_eq!(ready, "ready\n");
    let error = stop_pids(&[child.id().unwrap()]).await.unwrap_err();
    assert!(error
        .to_string()
        .contains("did not finish shutting down within 10s"));
    assert!(child.try_wait().unwrap().is_none());
    child.kill().await.unwrap();
    assert!(!child.wait().await.unwrap().success());
}
