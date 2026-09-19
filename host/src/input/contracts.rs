//! Adapter contract tests run without /dev/uinput or desktop services (T370).
use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);
impl Recorder {
    fn record(&self, value: String) {
        self.0.lock().unwrap().push(value);
    }
}
impl InputSink for Recorder {
    fn release_all(&self) {
        self.record("release".into());
    }
    fn touch(&self, position: (f64, f64, f64), action: u8, slot: u8) {
        self.record(format!("touch {position:?} {action} {slot}"));
    }
    fn pen(&self, sample: PenSample, enabled: bool) {
        self.record(format!(
            "pen {:?} {} {enabled}",
            sample.position, sample.action
        ));
    }
}
impl SettingsSink for Recorder {
    fn decoders(&self, _: crate::media::DecoderCapabilities) {}
    fn resolution(&self, pixels: (u32, u32), millimetres: (u32, u32)) {
        self.record(format!("resolution {pixels:?} {millimetres:?}"));
    }
    fn configure(&self, bitrate: Option<u32>, fps: Option<u32>, encoder: Option<String>) {
        self.record(format!("config {bitrate:?} {fps:?} {encoder:?}"));
    }
    fn mode(&self, pen_only: bool) {
        self.record(format!("mode {pen_only}"));
    }
}

#[test]
fn t484_old_decoder_receipt_cannot_acknowledge_new_encoder_packets() {
    let choice: uscreen_config::negotiation::DecoderChoice = serde_json::from_str(
        r#"{"name":"vendor.avc","stream":{"codec":"h264","profile":"baseline","level":41,"depth":8},"low_latency":false,"operating_rate":120}"#).unwrap();
    let tracker = crate::latency::LatencyTracker::new();
    let encoder =
        tracker.encoder_started_with_decoder("libx264", (640, 480, 60, 20000, 18), Some(choice));
    tracker.on_encoded_for(1, &encoder);
    let event = serde_json::from_str(
        r#"{"type":"rendered","seq":1,"decode_us":100,"decoder":"retired-decoder"}"#,
    )
    .unwrap();
    let sink = Recorder::default();
    handle_event(event, &sink, &sink, &tracker, true);
    assert_eq!(
        encoder.rendered(),
        0,
        "T484: producer identity cannot prove which decoder rendered"
    );
    let omitted = serde_json::from_str(r#"{"type":"rendered","seq":1,"decode_us":100}"#).unwrap();
    handle_event(omitted, &sink, &sink, &tracker, true);
    assert_eq!(
        encoder.rendered(),
        0,
        "T484: missing rich receipt became proof"
    );
    let matching = serde_json::from_str(r#"{"type":"rendered","seq":1,"decode_us":100,"decoder":"10:vendor.avc:h264:baseline:41:8:0:120"}"#).unwrap();
    handle_event(matching, &sink, &sink, &tracker, true);
    assert_eq!(encoder.rendered(), 1);
}

#[test]
fn t370_dispatch_preserves_order_and_controller_ownership_with_adapters() {
    let sink = Arc::new(Recorder::default());
    let settings = Recorder::default();
    let controllers = Arc::new(Controllers::new(sink.clone()));
    let first = controllers.claim();
    let latency = crate::latency::LatencyTracker::new();
    for text in [
        r#"{"type":"touch","x":0.2,"y":0.3,"pressure":0.4,"action":0,"slot":2}"#,
        r#"{"type":"resolution","width":1280,"height":800,"width_mm":240,"height_mm":150}"#,
        r#"{"type":"config","fps":45}"#,
        r#"{"type":"mode","pen_only":true}"#,
    ] {
        assert!(dispatch_controller_text(
            text,
            &controllers,
            first.id,
            &settings,
            &latency,
            true
        ));
    }
    let second = controllers.claim();
    assert!(!dispatch_controller_text(
        r#"{"type":"mode","pen_only":false}"#,
        &controllers,
        first.id,
        &settings,
        &latency,
        true,
    ));
    drop(first);
    assert_eq!(
        *sink.0.lock().unwrap(),
        ["release", "touch (0.2, 0.3, 0.4) 0 2", "release"]
    );
    assert_eq!(
        *settings.0.lock().unwrap(),
        [
            "resolution (1280, 800) (240, 150)",
            "config None Some(45) None",
            "mode true",
        ]
    );
    drop(second);
    assert_eq!(sink.0.lock().unwrap().last().unwrap(), "release");
}

struct FakeBackend {
    sink: Arc<Recorder>,
    started: Arc<tokio::sync::Notify>,
    stopped: Arc<tokio::sync::Notify>,
}
struct OnDrop(Arc<tokio::sync::Notify>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
impl InputBackend for FakeBackend {
    fn sink(&self) -> Arc<dyn InputSink> {
        self.sink.clone()
    }
    fn follow(
        &self,
        _tablet: watch::Receiver<bool>,
        _mode: watch::Receiver<bool>,
        _card: watch::Receiver<Option<u32>>,
        _config: InputConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let (started, stopped) = (self.started.clone(), self.stopped.clone());
        Box::pin(async move {
            let _cleanup = OnDrop(stopped);
            started.notify_one();
            std::future::pending::<()>().await;
        })
    }
}

#[tokio::test]
async fn t370_server_cancels_injected_backend_when_owner_ends() {
    let started = Arc::new(tokio::sync::Notify::new());
    let stopped = Arc::new(tokio::sync::Notify::new());
    let server = InputServer::with_backend(
        InputConfig {
            port: 0,
            ..InputConfig::default()
        },
        None,
        watch::channel(false).0,
        crate::latency::LatencyTracker::new(),
        Arc::new(tokio::sync::Notify::new()),
        (watch::channel(None).1, watch::channel(false).1),
        Arc::new(FakeBackend {
            sink: Arc::new(Recorder::default()),
            started: started.clone(),
            stopped: stopped.clone(),
        }),
    );
    let listener = server.bind().await.unwrap();
    let task = tokio::spawn(async move { server.run_with_listener(listener).await });
    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(1), stopped.notified())
        .await
        .unwrap();
}

fn t281_attachment() -> crate::attachment::Attachment {
    crate::session::Spec {
        capture: Default::default(),
        ports: (0, 0),
        token: Some("initial-t281-token".into()),
        devices: (false, false, false),
    }
    .prepare(watch::channel(false).0)
    .tablet
}

#[test]
fn t281_retired_socket_cannot_claim_or_dispatch_into_current_controller() {
    let tablet = t281_attachment();
    tablet.begin(Some("a".into()));
    let old = tablet.lease();
    tablet.begin(Some("b".into()));
    let current = tablet.lease();
    let recorder = Arc::new(Recorder::default());
    let controllers = Arc::new(Controllers::new(recorder.clone()));
    let (claim_mode, _) = watch::channel(false);
    let claim_settings = SessionSettings::new(&None, &claim_mode, false);
    let controller = claim_controller(&controllers, Some(&current), &claim_settings).unwrap();
    assert!(claim_controller(&controllers, Some(&old), &claim_settings).is_none());
    assert_eq!(*controllers.generation.borrow(), controller.id);
    let settings = Recorder::default();
    let latency = crate::latency::LatencyTracker::new();
    let mut dispatch = ControllerDispatch {
        controllers: &controllers,
        controller: controller.id,
        settings: &settings,
        latency: &latency,
        pen_enabled: true,
        attachment: Some(&old),
    };
    let resolution =
        r#"{"type":"resolution","width":1280,"height":800,"width_mm":240,"height_mm":150}"#;
    assert!(!dispatch.text(resolution));
    assert!(settings.0.lock().unwrap().is_empty());
    dispatch.attachment = Some(&current);
    assert!(dispatch.text(resolution));
    assert_eq!(
        *settings.0.lock().unwrap(),
        ["resolution (1280, 800) (240, 150)"]
    );
}

#[tokio::test]
async fn t281_socket_accepted_before_replacement_cannot_authenticate_after_it() {
    let tablet = t281_attachment();
    tablet.begin(Some("a".into()));
    let token = tablet.token().unwrap().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socket = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (accepted, _) = listener.accept().await.unwrap();
    let mut incoming = PendingInput::new(accepted);
    incoming.attachment = Some(tablet.lease());
    let recorder = Arc::new(Recorder::default());
    let controllers = Arc::new(Controllers::new(recorder.clone()));
    let task = tokio::spawn(handle_connection(
        incoming,
        InputConfig {
            token: Some(token.clone()),
            ..InputConfig::default()
        },
        None,
        watch::channel(false).0,
        crate::latency::LatencyTracker::new(),
        controllers,
        Arc::new(tokio::sync::Notify::new()),
    ));
    let (mut ws, _) = tokio_tungstenite::client_async("ws://localhost", socket)
        .await
        .unwrap();
    tablet.begin(Some("b".into()));
    ws.send(Message::Text(
        serde_json::json!({"type":"auth", "token":token}).to_string(),
    ))
    .await
    .unwrap();
    let reply = tokio::time::timeout(std::time::Duration::from_secs(1), ws.next())
        .await
        .unwrap();
    assert!(
        !matches!(reply, Some(Ok(Message::Text(_)))),
        "T281: retired socket got greeting"
    );
    task.await.unwrap().unwrap();
    assert!(
        recorder.0.lock().unwrap().is_empty(),
        "T281: retired socket claimed devices"
    );
}
