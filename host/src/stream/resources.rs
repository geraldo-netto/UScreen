//! T391: real loopback socket resource replay; no media decoding or desktop.
use super::*;
use crate::input::{InputConfig, InputServer};
use crate::media::EncoderGeneration;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration, Instant};

const FRAMES: usize = 120;
const PAYLOAD: usize = 64 * 1024;
const BASE: u32 = u32::MAX - 70;

type ServerTask = JoinHandle<Result<()>>;

#[derive(Default, Serialize)]
struct Readings {
    frames: usize,
    configs: usize,
    arrival_us: Vec<u64>,
}

#[derive(Clone, Copy, Serialize)]
struct Resources {
    fds: usize,
    tasks: usize,
    rss_kib: usize,
}

fn resources() -> Resources {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let rss = status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .unwrap();
    Resources {
        fds: std::fs::read_dir("/proc/self/fd").unwrap().count(),
        tasks: tokio::runtime::Handle::current()
            .metrics()
            .num_alive_tasks(),
        rss_kib: rss.split_whitespace().nth(1).unwrap().parse().unwrap(),
    }
}

struct Control {
    task: ServerTask,
    address: std::net::SocketAddr,
    _mode: watch::Sender<bool>,
    _card: watch::Sender<Option<u32>>,
    _tablet: watch::Sender<bool>,
    _client: tokio_tungstenite::WebSocketStream<TcpStream>,
}

async fn control() -> Control {
    let (mode, _) = watch::channel(false);
    let (card, card_rx) = watch::channel(None);
    let (tablet, tablet_rx) = watch::channel(false);
    let server = InputServer::new(
        InputConfig {
            port: 0,
            touch: false,
            pen: false,
            pointer: false,
            token: Some("a".repeat(64)),
            ..Default::default()
        },
        None,
        mode.clone(),
        crate::latency::LatencyTracker::new(),
        Default::default(),
        card_rx,
        tablet_rx,
    );
    let listener = server.bind().await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { server.run_with_listener(listener).await });
    let socket = TcpStream::connect(address).await.unwrap();
    let (mut client, _) = tokio_tungstenite::client_async("ws://localhost/", socket)
        .await
        .unwrap();
    let auth = serde_json::json!({"type":"auth", "token":"a".repeat(64)}).to_string();
    client
        .send(tokio_tungstenite::tungstenite::Message::Text(auth))
        .await
        .unwrap();
    assert!(client
        .next()
        .await
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap()
        .contains("connected"));
    Control {
        _client: client,
        task,
        address,
        _mode: mode,
        _card: card,
        _tablet: tablet,
    }
}

struct Session {
    server: Arc<StreamServer>,
    video: crate::video_queue::VideoSender,
    address: std::net::SocketAddr,
    task: ServerTask,
    control: Control,
    peers: Vec<TcpStream>,
    reader: JoinHandle<Readings>,
    starts: Arc<Mutex<Vec<Instant>>>,
    cache: Arc<Mutex<Option<Bytes>>>,
    config: Bytes,
    generation: EncoderGeneration,
}

async fn authenticated(address: std::net::SocketAddr, slow: bool) -> TcpStream {
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    if slow {
        socket.set_recv_buffer_size(1024).unwrap();
    }
    let mut socket = socket.connect(address).await.unwrap();
    socket.write_all("a".repeat(64).as_bytes()).await.unwrap();
    socket
}

async fn session() -> Session {
    let config = Bytes::from(vec![1]);
    let cache = Arc::new(Mutex::new(Some(config.clone())));
    let server = Arc::new(StreamServer::new(
        StreamConfig {
            video_port: 0,
            token: Some("a".repeat(64)),
        },
        cache.clone(),
        Default::default(),
    ));
    let listener = server.bind().await.unwrap();
    let address = listener.local_addr().unwrap();
    let (video, _) = crate::video_queue::channel(8, Default::default());
    let task = tokio::spawn({
        let (server, video) = (server.clone(), video.clone());
        async move { server.run_with_listener(video, listener).await }
    });
    let starts = Arc::new(Mutex::new(vec![Instant::now(); FRAMES]));
    let fast = authenticated(address, false).await;
    let reader = tokio::spawn(receive(fast, starts.clone()));
    let control = control().await;
    let peers = vec![
        authenticated(address, true).await,
        TcpStream::connect(address).await.unwrap(),
        TcpStream::connect(control.address).await.unwrap(),
    ];
    Session {
        server,
        video,
        address,
        task,
        control,
        peers,
        reader,
        starts,
        cache,
        config,
        generation: EncoderGeneration::new(),
    }
}

async fn receive(mut socket: TcpStream, starts: Arc<Mutex<Vec<Instant>>>) -> Readings {
    let mut report = Readings::default();
    let mut payload = vec![0; PAYLOAD];
    let mut config = 0;
    let mut waiting_idr = true;
    while let Ok(length) = socket.read_u32().await {
        let kind = socket.read_u8().await.unwrap();
        if kind == PACKET_TYPE_CONFIG {
            assert_eq!(length, 2);
            config = socket.read_u8().await.unwrap();
            waiting_idr = true;
            report.configs += 1;
            continue;
        }
        assert_eq!((kind, length as usize), (PACKET_TYPE_FRAME, PAYLOAD + 5));
        let seq = socket.read_u32().await.unwrap();
        socket.read_exact(&mut payload).await.unwrap();
        let index = seq.wrapping_sub(BASE) as usize;
        assert!(index < FRAMES);
        if waiting_idr {
            assert_eq!(index % 30, 0);
            waiting_idr = false;
        }
        assert!(payload.iter().all(|byte| *byte == config));
        report.frames += 1;
        report
            .arrival_us
            .push(starts.lock().unwrap()[index].elapsed().as_micros() as u64);
    }
    report
}

impl Session {
    fn publish(&mut self, index: usize) {
        if index == 60 {
            self.generation = EncoderGeneration::new();
            self.config = Bytes::from(vec![2]);
            *self.cache.lock().unwrap() = Some(self.config.clone());
        }
        self.starts.lock().unwrap()[index] = Instant::now();
        self.video
            .send(VideoPacket {
                data: Bytes::from(vec![self.config[0]; PAYLOAD]),
                seq: BASE.wrapping_add(index as u32),
                is_idr: index.is_multiple_of(30),
                codec_config: Some(self.config.clone()),
                generation: self.generation.active.clone(),
            })
            .ok()
            .expect("T391: normal fast/slow mix exhausted admission");
    }

    async fn churn(&self) {
        let mut denied = TcpStream::connect(self.address).await.unwrap();
        denied.write_all("b".repeat(64).as_bytes()).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), denied.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        let control = TcpStream::connect(self.control.address).await.unwrap();
        drop(control);
    }

    async fn finish(self) -> Readings {
        assert_eq!(
            self.video.receiver_count(),
            1,
            "T391: stalled viewer retained subscription"
        );
        self.server.stop();
        timeout(Duration::from_secs(1), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        self.control.task.abort();
        assert!(self.control.task.await.unwrap_err().is_cancelled());
        drop(self.peers);
        timeout(Duration::from_secs(1), self.reader)
            .await
            .unwrap()
            .unwrap()
    }
}

#[derive(Serialize)]
struct Trial {
    sessions: usize,
    before: Resources,
    peak: Resources,
    after: Resources,
    retained_peak_bytes: Vec<usize>,
    readers: Vec<Readings>,
    allocations: crate::allocation_probe::Counts,
}

async fn traffic(sessions: &mut [Session]) -> (Resources, Vec<usize>) {
    // Both admitted viewers must subscribe before the first IDR is published.
    timeout(Duration::from_secs(1), async {
        while sessions
            .iter()
            .any(|session| session.video.receiver_count() != 2)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut peak = resources();
    let mut timer = tokio::time::interval(Duration::from_millis(17));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    for index in 0..FRAMES {
        timer.tick().await;
        for session in sessions.iter_mut() {
            session.publish(index);
            if index.is_multiple_of(15) {
                session.churn().await;
            }
        }
        update_peak(&mut peak, resources());
    }
    tokio::time::sleep(Duration::from_millis(30)).await;
    let retained = sessions
        .iter()
        .map(|session| session.video.usage().1)
        .collect();
    (peak, retained)
}

fn update_peak(peak: &mut Resources, current: Resources) {
    peak.fds = peak.fds.max(current.fds);
    peak.tasks = peak.tasks.max(current.tasks);
    peak.rss_kib = peak.rss_kib.max(current.rss_kib);
}

async fn trial(count: usize) -> Trial {
    let before = resources();
    let mut sessions = Vec::new();
    for _ in 0..count {
        sessions.push(session().await);
    }
    let ((peak, retained_peak_bytes), allocations) =
        crate::allocation_probe::measure_async(traffic(&mut sessions)).await;
    let mut readers = Vec::new();
    for session in sessions {
        readers.push(session.finish().await);
    }
    tokio::task::yield_now().await;
    let after = resources();
    assert!(
        after.fds <= before.fds,
        "T391: socket leak after churn/shutdown"
    );
    assert_eq!(after.tasks, before.tasks, "T391: task leak after shutdown");
    assert!(readers
        .iter()
        .all(|reader| reader.frames >= 90 && reader.configs == 2));
    assert!(retained_peak_bytes
        .iter()
        .all(|bytes| *bytes <= crate::video_queue::RETAINED_BYTES));
    Trial {
        sessions: count,
        before,
        peak,
        after,
        retained_peak_bytes,
        readers,
        allocations,
    }
}

fn isolated() -> bool {
    if std::env::var_os("USCREEN_T391_REPLAY_CHILD").is_some() {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "stream::resources::t391_one_two_four_sessions_release_resources",
            "--nocapture",
        ])
        .env("USCREEN_T391_REPLAY_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    true
}

#[tokio::test]
async fn t391_one_two_four_sessions_release_resources() {
    if isolated() {
        return;
    }
    for count in [1, 2, 4] {
        println!(
            "T391 resources {}",
            serde_json::to_string(&trial(count).await).unwrap()
        );
    }
}
