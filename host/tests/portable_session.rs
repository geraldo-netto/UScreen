//! T523: real protocol sockets with injected capture/input, independent of OS devices.
use futures_util::{SinkExt, StreamExt};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uscreen::{
    input::{
        backend::{InputBackend, InputSink, PenSample},
        InputConfig,
    },
    media::{EncoderGeneration, EncoderSettings, VideoPacket},
    media_storage::MediaBytes,
    session::{CaptureBackend, CaptureContext, CaptureResources, CaptureWorkers, Prepared, Spec},
};

#[derive(Default)]
struct State {
    starts: AtomicUsize,
    capture_done: AtomicBool,
    input_done: AtomicBool,
}
struct Capture {
    state: Arc<State>,
    resources: CaptureResources,
    frames: mpsc::Receiver<VideoPacket>,
}
struct Input(Arc<State>);
struct Complete(Arc<State>, bool);
impl Drop for Complete {
    fn drop(&mut self) {
        let done = if self.1 {
            &self.0.capture_done
        } else {
            &self.0.input_done
        };
        done.store(true, Ordering::Release);
    }
}
impl CaptureBackend for Capture {
    fn resources(&self) -> CaptureResources {
        self.resources.clone()
    }
    fn start(self: Box<Self>, mut context: CaptureContext) -> CaptureWorkers {
        self.state.starts.fetch_add(1, Ordering::Relaxed);
        let mut frames = self.frames;
        let complete = Complete(self.state, true);
        let capture = tokio::spawn(async move {
            let _complete = complete;
            loop {
                tokio::select! {
                    _ = context.stop.wait_for(|stop| *stop) => return,
                    packet = frames.recv() => match packet {
                        Some(packet) => { let _ = context.video.send(packet); },
                        None => return,
                    },
                }
            }
        });
        CaptureWorkers {
            capture,
            auxiliary: vec![],
        }
    }
}
impl InputSink for Input {
    fn release_all(&self) {}
    fn touch(&self, _: (f64, f64, f64), _: u8, _: u8) {
        panic!("disabled touch reached native sink");
    }
    fn pen(&self, _: PenSample, _: bool) {
        panic!("disabled pen reached native sink");
    }
}
impl InputBackend for Input {
    fn sink(&self) -> Arc<dyn InputSink> {
        Arc::new(Input(self.0.clone()))
    }
    fn follow(
        &self,
        _: watch::Receiver<bool>,
        _: watch::Receiver<bool>,
        _: InputConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let complete = Complete(self.0.clone(), false);
        Box::pin(async move {
            let _complete = complete;
            std::future::pending().await
        })
    }
}

fn prepare(ports: (u16, u16), state: Arc<State>) -> (Prepared, mpsc::Sender<VideoPacket>) {
    let resources = CaptureResources::default();
    resources
        .codec_config
        .publish(Some(MediaBytes::from_static(b"csd")));
    let (sender, frames) = mpsc::channel(2);
    let settings = EncoderSettings {
        encoder: "libx264".into(),
        fps: 30,
        bitrate: 20000,
        width: 640,
        height: 480,
        quality: 18,
        width_mm: 310,
        height_mm: 194,
        stream_scale: 1,
        geometry_ready: false,
        decoders: None,
        decoder_epoch: 0,
        selection: None,
    };
    let spec = Spec {
        settings,
        instance: 0,
        ports,
        token: Some("a".repeat(64)),
        devices: (false, false, false),
    };
    (
        spec.prepare(
            watch::channel(false).0,
            Box::new(Capture {
                state: state.clone(),
                resources,
                frames,
            }),
            Arc::new(Input(state)),
        ),
        sender,
    )
}

async fn ports() -> (u16, u16) {
    let video = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let input = TcpListener::bind("127.0.0.1:0").await.unwrap();
    (
        video.local_addr().unwrap().port(),
        input.local_addr().unwrap().port(),
    )
}

async fn packet(socket: &mut TcpStream) -> Vec<u8> {
    let length = socket.read_u32().await.unwrap() as usize;
    assert!(length <= 1024, "T523: unexpected wire length");
    let mut payload = vec![0; length];
    socket.read_exact(&mut payload).await.unwrap();
    payload
}

#[tokio::test]
async fn t523_shared_session_authenticates_streams_and_joins_native_workers() {
    let ports = ports().await;
    let state = Arc::new(State::default());
    let (prepared, frames) = prepare(ports, state.clone());
    let tracker = prepared.resources.latency.clone();
    let evidence = tracker.encoder_started("libx264", (640, 480, 30, 20000, 18));
    let _activity = tracker.encoder_activity(evidence.clone());
    let (_daemon, shutdown) = watch::channel(false);
    assert_eq!(state.starts.load(Ordering::Relaxed), 0);
    let runtime = prepared.start(shutdown).await.unwrap();
    assert_eq!(state.starts.load(Ordering::Relaxed), 1);
    let (mut control, _) = connect_async(format!("ws://127.0.0.1:{}", ports.1))
        .await
        .unwrap();
    control
        .send(Message::Text(
            serde_json::json!({"type":"auth","token":"a".repeat(64)}).to_string(),
        ))
        .await
        .unwrap();
    let greeting = control.next().await.unwrap().unwrap().into_text().unwrap();
    let greeting: serde_json::Value = serde_json::from_str(&greeting).unwrap();
    assert_eq!(greeting["touch"], false);
    assert_eq!(greeting["pen"], false);
    let mut video = TcpStream::connect(("127.0.0.1", ports.0)).await.unwrap();
    video.write_all("a".repeat(64).as_bytes()).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), packet(&mut video))
            .await
            .unwrap(),
        b"\x00csd"
    );
    let generation = EncoderGeneration::new();
    tracker.on_encoded_for(u32::MAX, &evidence);
    frames
        .send(VideoPacket {
            data: MediaBytes::from_static(b"picture"),
            is_idr: true,
            seq: u32::MAX,
            codec_config: Some(MediaBytes::from_static(b"csd")),
            generation: generation.active.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), packet(&mut video))
            .await
            .unwrap(),
        b"\x01\xff\xff\xff\xffpicture"
    );
    control
        .send(Message::Text(
            serde_json::json!({"type":"rendered","seq":u32::MAX,"decode_us":100}).to_string(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while evidence.rendered() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(evidence.unrendered_output(), 0);
    runtime.tablet_tx.begin(Some("replacement".into()));
    let mut byte = [0];
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), video.read(&mut byte)).await,
        Ok(Ok(0)) | Ok(Err(_))
    ));
    runtime.stop().await;
    assert!(state.capture_done.load(Ordering::Acquire));
    assert!(state.input_done.load(Ordering::Acquire));
    TcpListener::bind(("127.0.0.1", ports.0)).await.unwrap();
    TcpListener::bind(("127.0.0.1", ports.1)).await.unwrap();
}

#[tokio::test]
async fn t523_bind_failure_starts_no_backend_and_releases_first_listener() {
    let input = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ports = (ports().await.0, input.local_addr().unwrap().port());
    let state = Arc::new(State::default());
    let (prepared, _frames) = prepare(ports, state.clone());
    let (_daemon, shutdown) = watch::channel(false);
    assert!(prepared.start(shutdown).await.is_err());
    assert_eq!(state.starts.load(Ordering::Relaxed), 0);
    TcpListener::bind(("127.0.0.1", ports.0)).await.unwrap();
}

#[tokio::test]
async fn t523_dropping_session_cancels_both_native_workers() {
    let state = Arc::new(State::default());
    let (prepared, _frames) = prepare(ports().await, state.clone());
    let (_daemon, shutdown) = watch::channel(false);
    let runtime = prepared.start(shutdown).await.unwrap();
    tokio::task::yield_now().await;
    drop(runtime);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !(state.capture_done.load(Ordering::Acquire)
            && state.input_done.load(Ordering::Acquire))
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
