//! T525: ordinary Windows daemon invokes a native, isolated ADB fixture.
#![cfg(windows)]
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
struct Daemon(std::process::Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(condition: impl Fn() -> bool) {
    blent_config::lifecycle::wait_until(Duration::from_secs(10), || Ok(condition())).unwrap();
}
#[path = "support/usb_adb.rs"]
mod usb_fixture;
use usb_fixture::fixture;
#[tokio::test]
async fn t525_native_daemon_discovers_usb_without_host_shell() {
    let root = tempfile::tempdir().unwrap();
    let adb = root.path().join("ADB café 東京 & spaces");
    std::fs::create_dir(&adb).unwrap();
    fixture(&adb);
    let runtime = root.path().join("runtime");
    let mut path = vec![adb.clone()];
    path.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let video = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let input = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ports = (
        video.local_addr().unwrap().port(),
        input.local_addr().unwrap().port(),
    );
    drop((video, input));
    let mut daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_blent"))
            .env("PATH", std::env::join_paths(path).unwrap())
            .arg("--video-port")
            .arg(ports.0.to_string())
            .arg("--input-port")
            .arg(ports.1.to_string())
            .arg("--runtime-dir")
            .arg(&runtime)
            .arg("start")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait(|| adb.join("invoked").exists());
    wait(|| {
        blent_config::windows::lifecycle::load_sessions(&runtime)
            .is_some_and(|sessions| sessions.len() == 1)
    });
    let sessions = blent_config::windows::lifecycle::load_sessions(&runtime).unwrap();
    assert_eq!((sessions[0].video_port, sessions[0].input_port), ports);
    assert_eq!(
        blent::adb_inventory::query(adb.join("adb.exe").to_str().unwrap()).await,
        Some(vec!["USB".into()])
    );
    let status = Command::new(env!("CARGO_BIN_EXE_blent"))
        .arg("--runtime-dir")
        .arg(&runtime)
        .arg("status")
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("USB prepared: USB"));
    let first = token(&adb);
    let mut socket = authenticate(ports.1, &first).await;
    use futures_util::StreamExt;
    let greeting = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    let greeting: serde_json::Value = serde_json::from_str(&greeting).unwrap();
    assert_eq!(greeting["transport"], "usb");
    assert_eq!(greeting["pen"], false);
    assert_eq!(greeting["touch"], false);
    std::fs::write(
        adb.join("inventory"),
        "List of devices attached\nUSB\toffline\n",
    )
    .unwrap();
    wait(|| {
        blent_config::windows::lifecycle::load_sessions(&runtime)
            .is_some_and(|sessions| sessions.is_empty())
    });
    assert!(std::fs::read_to_string(adb.join("routes"))
        .unwrap()
        .is_empty());
    std::fs::write(
        adb.join("inventory"),
        "List of devices attached\nUSB\tdevice\n",
    )
    .unwrap();
    wait(|| {
        blent_config::windows::lifecycle::load_sessions(&runtime)
            .is_some_and(|sessions| sessions.len() == 1)
    });
    assert_ne!(token(&adb), first);
    let mut stale = authenticate(ports.1, &first).await;
    let response = tokio::time::timeout(Duration::from_secs(2), stale.next())
        .await
        .unwrap();
    assert!(!matches!(
        response,
        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_)))
    ));
    blent_config::windows::lifecycle::stop(&runtime, Duration::from_secs(10)).unwrap();
    wait(|| !runtime.join("daemon.json").exists());
    assert!(daemon.0.wait().unwrap().success());
    assert!(std::fs::read_to_string(adb.join("routes"))
        .unwrap()
        .is_empty());
    assert!(!runtime.join("sessions.json").exists());
}

fn token(root: &Path) -> String {
    std::fs::read_to_string(root.join("delivered"))
        .unwrap()
        .split(" --es token ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .into()
}
async fn authenticate(
    port: u16,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use futures_util::SinkExt;
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .unwrap();
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"type":"auth","token":token}).to_string(),
        ))
        .await
        .unwrap();
    socket
}
