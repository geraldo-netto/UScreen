//! T673: the daemon's USB/session pipeline delivers only authenticated native input.
#![cfg(windows)]
use blent::usb::{monitor::Monitor, Adb, NativeCommands};
use blent_config::{
    direct_input::{Config, Mode},
    input_mapping::MonitorInventory,
    storage::ConfigStore,
    windows::monitors::NativeInventory,
    FileConfig,
};
use futures_util::{SinkExt, StreamExt};
use std::{path::Path, sync::atomic::Ordering, time::Duration};
use tokio::sync::watch;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
#[path = "support/usb_adb.rs"]
mod fixture;
#[path = "../../testdata/input_window.rs"]
mod window;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn ports() -> (u16, u16) {
    let video = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let input = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    (
        video.local_addr().unwrap().port(),
        input.local_addr().unwrap().port(),
    )
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
async fn receive(socket: &mut Socket) -> Message {
    tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}
async fn connect(port: u16, token: &str) -> Socket {
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            serde_json::json!({"type":"auth","token":token}).to_string(),
        ))
        .await
        .unwrap();
    socket
}
async fn command(socket: &mut Socket, command: serde_json::Value) -> serde_json::Value {
    socket
        .send(Message::Text(
            serde_json::json!({"type":"direct_input","command":command}).to_string(),
        ))
        .await
        .unwrap();
    serde_json::from_str(&receive(socket).await.into_text().unwrap()).unwrap()
}
async fn down(socket: &mut Socket, kind: &str) {
    let event = serde_json::json!({"type":kind,"x":0.5,"y":0.5,"phase":"down"});
    let mut event = event.as_object().unwrap().clone();
    if kind == "mouse" {
        event.insert("button".into(), "left".into());
    } else {
        event.insert("slot".into(), 0.into());
    }
    socket
        .send(Message::Text(
            serde_json::json!({"type":"direct_input","command":{"type":"event","event":event}})
                .to_string(),
        ))
        .await
        .unwrap();
    socket.send(Message::Ping(vec![1])).await.unwrap();
    assert!(matches!(receive(socket).await, Message::Pong(_)));
    window::pump();
}

#[tokio::test]
async fn t673_native_usb_session_delivers_replaces_reconnects_and_cleans_owned_input() {
    let snapshot = NativeInventory.snapshot().unwrap();
    let display = &snapshot.monitors()[0];
    let (x, y) = display.project(0.5, 0.5).unwrap();
    let _window = window::Window::new(x, y);
    window::pump();
    let root = tempfile::tempdir().unwrap();
    fixture::fixture(root.path());
    let (video_port, input_port) = ports();
    let config = FileConfig {
        video_port,
        input_port,
        max_tablets: 1,
        input_pen: false,
        input_pointer: false,
        check_updates: false,
        direct_input: Some(Config {
            monitor: display.id.clone(),
            mode: Mode::DirectMouse,
        }),
        ..Default::default()
    };
    let store = ConfigStore::new(root.path().join("input.toml"));
    store
        .update(|saved| {
            *saved = config.clone();
            Ok(())
        })
        .unwrap();
    let (_stop, stop) = watch::channel(false);
    let mut monitor = Monitor::new(
        Adb(NativeCommands(root.path().join("adb.exe"))),
        config,
        stop,
    )
    .unwrap()
    .with_network(store.clone());
    monitor.poll().await;
    assert_eq!(monitor.sessions().len(), 1);
    let first = token(root.path());
    let mut denied = connect(input_port, "wrong").await;
    assert!(matches!(receive(&mut denied).await, Message::Close(_)));
    let mut socket = connect(input_port, &first).await;
    let greeting: serde_json::Value =
        serde_json::from_str(&receive(&mut socket).await.into_text().unwrap()).unwrap();
    assert_eq!(greeting["pen"], false);
    assert_eq!(greeting["transport"], "usb");
    assert_eq!(greeting["direct_input"]["negotiated"], false);
    assert_eq!(
        command(
            &mut socket,
            serde_json::json!({"type":"negotiate","version":1})
        )
        .await["direct_input"]["negotiated"],
        true
    );
    let before = window::DOWN.load(Ordering::SeqCst);
    down(&mut socket, "mouse").await;
    assert!(window::DOWN.load(Ordering::SeqCst) > before);
    let up = window::UP.load(Ordering::SeqCst);
    let mut replacement = connect(input_port, &first).await;
    receive(&mut replacement).await;
    window::pump();
    assert!(
        window::UP.load(Ordering::SeqCst) > up,
        "T673: replacement left mouse held"
    );
    command(
        &mut replacement,
        serde_json::json!({"type":"negotiate","version":1}),
    )
    .await;
    command(
        &mut replacement,
        serde_json::json!({"type":"select","mode":"touch"}),
    )
    .await;
    assert_eq!(store.load().direct_input.unwrap().mode, Mode::Touch);
    down(&mut replacement, "touch").await;
    let lifted = window::TOUCH_UP.load(Ordering::SeqCst);
    std::fs::write(
        root.path().join("inventory"),
        "List of devices attached\nUSB\toffline\n",
    )
    .unwrap();
    monitor.poll().await;
    window::pump();
    assert!(
        window::TOUCH_UP.load(Ordering::SeqCst) > lifted,
        "T673: disconnect left touch held"
    );
    assert!(monitor.sessions().is_empty());
    std::fs::write(
        root.path().join("inventory"),
        "List of devices attached\nUSB\tdevice\n",
    )
    .unwrap();
    monitor.poll().await;
    assert_ne!(token(root.path()), first);
    let mut stale = connect(input_port, &first).await;
    assert!(matches!(receive(&mut stale).await, Message::Close(_)));
    let mut reconnected = connect(input_port, &token(root.path())).await;
    let greeting: serde_json::Value =
        serde_json::from_str(&receive(&mut reconnected).await.into_text().unwrap()).unwrap();
    assert_eq!(greeting["direct_input"]["mode"], "touch");
    command(
        &mut reconnected,
        serde_json::json!({"type":"negotiate","version":1}),
    )
    .await;
    down(&mut reconnected, "touch").await;
    let lifted = window::TOUCH_UP.load(Ordering::SeqCst);
    monitor.shutdown().await.unwrap();
    window::pump();
    assert!(
        window::TOUCH_UP.load(Ordering::SeqCst) > lifted,
        "T673: shutdown left touch held"
    );
    assert!(std::fs::read_to_string(root.path().join("routes"))
        .unwrap()
        .is_empty());
}
