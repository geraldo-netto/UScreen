use super::*;
use blent_config::direct_input::Button;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn t673_native_authenticated_session_delivers_and_retires_in_owned_window() {
    let snapshot = NativeInventory.snapshot().unwrap();
    let monitor = &snapshot.monitors()[0];
    let (x, y) = monitor.project(0.5, 0.5).unwrap();
    let _window = window::Window::new(x, y);
    window::pump();
    let directory = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(directory.path().join("input.toml"));
    let config = Config {
        monitor: monitor.id.clone(),
        mode: Mode::DirectMouse,
    };
    store
        .update(|saved| {
            saved.direct_input = Some(config.clone());
            Ok(())
        })
        .unwrap();
    let backend = native(config, true, true, store.clone()).unwrap();
    let (_tablet, tablet) = watch::channel(true);
    let (mode, _) = watch::channel(true);
    let server = crate::input::InputServer::with_backend(
        InputConfig {
            port: 0,
            token: Some("native-test".into()),
            touch: false,
            pen: false,
            pointer: false,
            ..Default::default()
        },
        None,
        mode,
        Default::default(),
        Arc::new(tokio::sync::Notify::new()),
        tablet,
        Arc::new(backend),
    );
    let listener = server.bind().await.unwrap();
    let address = format!("ws://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { server.run_with_listener(listener).await });
    let (mut socket, _) = tokio_tungstenite::connect_async(address).await.unwrap();
    socket
        .send(Message::Text(
            r#"{"type":"auth","token":"native-test"}"#.into(),
        ))
        .await
        .unwrap();
    socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            r#"{"type":"direct_input","command":{"type":"negotiate","version":1}}"#.into(),
        ))
        .await
        .unwrap();
    socket.next().await.unwrap().unwrap();
    let before = window::DOWN.load(Ordering::SeqCst);
    let command = Command::Event {
        event: Event::Mouse {
            x: 0.5,
            y: 0.5,
            button: Some(Button::Left),
            phase: Phase::Down,
        },
    };
    socket
        .send(Message::Text(
            serde_json::json!({"type":"direct_input","command":command}).to_string(),
        ))
        .await
        .unwrap();
    socket.send(Message::Ping(vec![1])).await.unwrap();
    socket.next().await.unwrap().unwrap();
    window::pump();
    assert!(
        window::DOWN.load(Ordering::SeqCst) > before,
        "T673: mouse did not reach owned window"
    );
    let up = window::UP.load(Ordering::SeqCst);
    socket
        .send(Message::Text(
            r#"{"type":"direct_input","command":{"type":"select","mode":"touch"}}"#.into(),
        ))
        .await
        .unwrap();
    socket.next().await.unwrap().unwrap();
    window::pump();
    assert!(
        window::UP.load(Ordering::SeqCst) > up,
        "T673: mode switch left mouse held"
    );
    assert_eq!(store.load().direct_input.unwrap().mode, Mode::Touch);
    let touch = window::TOUCH.load(Ordering::SeqCst);
    for action in [0, 2, 1, 0] {
        socket.send(Message::Text(serde_json::json!({"type":"touch","x":0.5,"y":0.5,"pressure":1,"action":action,"slot":0}).to_string())).await.unwrap();
        socket.send(Message::Ping(vec![2])).await.unwrap();
        assert!(matches!(socket.next().await, Some(Ok(Message::Pong(_)))));
        window::pump();
    }
    assert!(
        window::TOUCH.load(Ordering::SeqCst) > touch,
        "T673: touch did not reach owned window"
    );
    socket.close(None).await.unwrap();
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    window::pump();
    task.abort();
    let _ = task.await;
    let native = Native {
        store: store.clone(),
    };
    assert!(native
        .save(&Config {
            monitor: "missing".into(),
            mode: Mode::Touch
        })
        .is_err());
    assert_eq!(store.load().direct_input.unwrap().monitor, monitor.id);
    let restarted = super::native(
        Config {
            monitor: monitor.id.clone(),
            mode: Mode::DirectMouse,
        },
        true,
        true,
        store,
    )
    .unwrap();
    assert_eq!(
        restarted.sink().direct_status().unwrap().mode,
        Mode::Touch,
        "T673: reconnect lost saved input mode"
    );
}
