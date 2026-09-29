use super::*;
use blent_config::{
    direct_input::{Button, Contact, MouseAction},
    input_mapping::{Monitor, Rect, Rotation},
};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

#[derive(Default)]
struct Evidence {
    calls: Mutex<Vec<String>>,
    width: AtomicI32,
    fail: AtomicBool,
}
struct Fake(Arc<Evidence>);
struct Device(Arc<Evidence>);
impl Adapter for Device {
    fn touch(&mut self, frame: &[Contact]) -> Result<()> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("touch {frame:?}"));
        ensure!(!self.0.fail.load(Ordering::SeqCst), "denied");
        Ok(())
    }
    fn mouse(&mut self, point: (u16, u16), action: MouseAction) -> Result<()> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("mouse {point:?} {action:?}"));
        ensure!(!self.0.fail.load(Ordering::SeqCst), "denied");
        Ok(())
    }
    fn retire(&mut self) -> Result<()> {
        self.0.calls.lock().unwrap().push("retire".into());
        Ok(())
    }
}
impl Environment for Fake {
    type Device = Device;
    fn snapshot(&self) -> Result<Snapshot> {
        Snapshot::new(vec![Monitor {
            id: "screen".into(),
            name: "Screen".into(),
            bounds: Rect {
                left: 0,
                top: 0,
                right: 100 + self.0.width.load(Ordering::SeqCst),
                bottom: 100,
            },
            rotation: Rotation::Identity,
            scale_percent: 100,
            primary: true,
        }])
    }
    fn create(&self, mode: Mode) -> Result<Device> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("create {mode:?}"));
        Ok(Device(self.0.clone()))
    }
    fn save(&self, config: &Config) -> Result<()> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("save {:?}", config.mode));
        Ok(())
    }
}
fn backend(mode: Mode, touch: bool, mouse: bool) -> (Backend, Arc<Evidence>) {
    let evidence = Arc::new(Evidence::default());
    let factory = evidence.clone();
    (
        Backend::new(
            Config {
                monitor: "screen".into(),
                mode,
            },
            touch,
            mouse,
            move || Fake(factory),
        )
        .unwrap(),
        evidence,
    )
}
fn mouse(phase: Phase, button: Option<Button>, x: f64) -> Command {
    Command::Event {
        event: Event::Mouse {
            x,
            y: 0.5,
            phase,
            button,
        },
    }
}
fn negotiate(sink: &dyn InputSink) {
    sink.direct(Command::Negotiate { version: 1 }).unwrap();
}

#[tokio::test]
async fn t673_tablet_and_mode_changes_retire_negotiation_before_reuse() {
    let (backend, _) = backend(Mode::DirectMouse, true, true);
    let sink = backend.sink();
    let (tablet, tablet_rx) = watch::channel(true);
    let (mode, mode_rx) = watch::channel(true);
    let task = tokio::spawn(backend.follow(tablet_rx, mode_rx, InputConfig::default()));
    negotiate(&sink);
    tablet.send(false).unwrap();
    wait_retired(&*sink).await;
    assert!(sink.direct(mouse(Phase::Move, None, 0.5)).is_err());
    tablet.send(true).unwrap();
    tokio::task::yield_now().await;
    negotiate(&sink);
    mode.send(false).unwrap();
    wait_retired(&*sink).await;
    negotiate(&sink);
    drop(mode);
    tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(!sink.direct_status().unwrap().negotiated);
}

async fn wait_retired(sink: &dyn InputSink) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while sink.direct_status().unwrap().negotiated {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn t673_wire_dispatch_accepts_negotiated_touch_and_rejects_pen_and_failed_input() {
    let (backend, evidence) = backend(Mode::Touch, true, true);
    let sink = backend.sink();
    let (mode, _) = watch::channel(true);
    let settings_tx = None;
    let settings = super::super::SessionSettings::new(&settings_tx, &mode, false);
    let latency = crate::latency::LatencyTracker::new();
    let dispatch = |text: &str| {
        super::super::dispatch_event(
            serde_json::from_str(text).unwrap(),
            &*sink,
            &settings,
            &latency,
            false,
        )
    };
    assert!(dispatch(
        r#"{"type":"direct_input","command":{"type":"negotiate","version":1}}"#
    ));
    for action in [0, 2, 1] {
        assert!(dispatch(&format!(
            r#"{{"type":"touch","x":0.5,"y":0.5,"pressure":1,"action":{action},"slot":0}}"#
        )));
    }
    assert!(!dispatch(
        r#"{"type":"pen","x":0.5,"y":0.5,"pressure":1,"tilt_x":0,"tilt_y":0,"action":0}"#
    ));
    evidence.fail.store(true, Ordering::SeqCst);
    assert!(!dispatch(
        r#"{"type":"direct_input","command":{"type":"event","event":{"type":"touch","x":0.5,"y":0.5,"slot":0,"phase":"down"}}}"#
    ));
    assert!(!sink.direct_status().unwrap().negotiated);
}

#[test]
fn t673_negotiation_modes_bounds_and_failed_delivery_retire_owned_input() {
    let (backend, e) = backend(Mode::DirectMouse, true, true);
    let sink = backend.sink();
    assert!(!sink.direct_status().unwrap().negotiated);
    assert!(sink.direct(mouse(Phase::Move, None, 0.5)).is_err());
    assert!(e.calls.lock().unwrap().is_empty());
    for version in [0, 2, 255, u32::MAX] {
        assert!(sink.direct(Command::Negotiate { version }).is_err());
    }
    negotiate(&sink);
    sink.direct(mouse(Phase::Down, Some(Button::Left), 0.5))
        .unwrap();
    sink.direct(mouse(Phase::Move, None, 0.75)).unwrap();
    sink.direct(Command::Select { mode: Mode::Touch }).unwrap();
    let calls = e.calls.lock().unwrap().clone();
    assert!(
        calls.iter().position(|v| v == "retire").unwrap()
            < calls.iter().position(|v| v == "create Touch").unwrap()
    );
    sink.touch((0.5, 0.5, 1.0), 0, 0);
    sink.touch((0.75, 0.5, 1.0), 2, 0);
    sink.touch((0.75, 0.5, 0.0), 1, 0);
    assert!(e.calls.lock().unwrap().iter().any(|v| v.contains("touch")));
    sink.touch((0.5, 0.5, 1.0), 255, 255);
    assert!(!sink.direct_status().unwrap().negotiated);
    for x in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        1.01,
        f64::MAX,
    ] {
        negotiate(&sink);
        sink.direct(Command::Select {
            mode: Mode::DirectMouse,
        })
        .unwrap();
        assert!(sink.direct(mouse(Phase::Move, None, x)).is_err());
        assert!(!sink.direct_status().unwrap().negotiated);
    }
    negotiate(&sink);
    e.fail.store(true, Ordering::SeqCst);
    assert!(sink
        .direct(mouse(Phase::Down, Some(Button::Right), 0.0))
        .is_err());
    e.fail.store(false, Ordering::SeqCst);
    negotiate(&sink);
    e.width.store(10, Ordering::SeqCst);
    assert!(sink.direct(mouse(Phase::Move, None, 1.0)).is_err());
    negotiate(&sink);
    sink.direct(mouse(Phase::Move, None, 1.0)).unwrap();
    sink.release_all();
    sink.release_all();
    assert!(sink.direct(Command::Select { mode: Mode::Touch }).is_err());
    sink.pen(
        PenSample {
            position: (0.0, 0.0, 0.0),
            tilt: (0.0, 0.0),
            eraser: false,
            action: 0,
            button: None,
        },
        true,
    );
}

#[test]
fn t673_disabled_modes_and_seeded_wire_values_never_reach_devices() {
    for mode in [Mode::Touch, Mode::DirectMouse] {
        let (backend, e) = backend(mode, false, false);
        let sink = backend.sink();
        assert!(sink.direct(Command::Negotiate { version: 1 }).is_err());
        assert!(e.calls.lock().unwrap().is_empty());
    }
    for value in -32..=300 {
        let text = format!(
            r#"{{"type":"event","event":{{"type":"touch","x":0,"y":1,"slot":{value},"phase":"down"}}}}"#
        );
        let parsed = serde_json::from_str::<Command>(&text);
        assert_eq!(parsed.is_ok(), (0..=255).contains(&value));
        if let Ok(command) = parsed {
            let (backend, _) = backend(Mode::Touch, true, false);
            let sink = backend.sink();
            negotiate(&sink);
            assert_eq!(sink.direct(command).is_ok(), value < 10);
        }
    }
    for text in [
        r#"{"type":"negotiate","version":-1}"#,
        r#"{"type":"select","mode":"pen"}"#,
        r#"{"type":"event","event":{"type":"mouse","phase":"down","button":"extra","x":0,"y":0}}"#,
    ] {
        assert!(serde_json::from_str::<Command>(text).is_err());
    }
}

#[tokio::test]
async fn t673_authenticated_owner_replacement_and_disconnect_retire_buttons() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let (backend, e) = backend(Mode::DirectMouse, true, true);
    let (_tablet, tablet) = watch::channel(true);
    let (mode, _) = watch::channel(true);
    let server = super::super::InputServer::with_backend(
        InputConfig {
            port: 0,
            token: Some("secret".into()),
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
    let (mut denied, _) = tokio_tungstenite::connect_async(&address).await.unwrap();
    denied
        .send(Message::Text(r#"{"type":"auth","token":"wrong"}"#.into()))
        .await
        .unwrap();
    assert!(matches!(denied.next().await, Some(Ok(Message::Close(_)))));
    assert!(e.calls.lock().unwrap().is_empty());
    let (mut first, _) = tokio_tungstenite::connect_async(&address).await.unwrap();
    first
        .send(Message::Text(r#"{"type":"auth","token":"secret"}"#.into()))
        .await
        .unwrap();
    let greeting = first.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(greeting.contains("direct_input"));
    first
        .send(Message::Text(
            r#"{"type":"direct_input","command":{"type":"negotiate","version":1}}"#.into(),
        ))
        .await
        .unwrap();
    assert!(first
        .next()
        .await
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap()
        .contains("\"negotiated\":true"));
    first.send(Message::Text(serde_json::json!({"type":"direct_input","command":mouse(Phase::Down,Some(Button::Left),0.5)}).to_string())).await.unwrap();
    first.send(Message::Ping(vec![1])).await.unwrap();
    assert!(matches!(first.next().await, Some(Ok(Message::Pong(_)))));
    let (mut second, _) = tokio_tungstenite::connect_async(&address).await.unwrap();
    second
        .send(Message::Text(r#"{"type":"auth","token":"secret"}"#.into()))
        .await
        .unwrap();
    second.next().await.unwrap().unwrap();
    assert!(e.calls.lock().unwrap().iter().any(|call| call == "retire"));
    second
        .send(Message::Text(
            r#"{"type":"direct_input","command":{"type":"negotiate","version":1}}"#.into(),
        ))
        .await
        .unwrap();
    second.next().await.unwrap().unwrap();
    second.close(None).await.unwrap();
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    task.abort();
    let _ = task.await;
    let calls = e.calls.lock().unwrap();
    assert!(calls.iter().filter(|call| *call == "retire").count() >= 2);
}
