//! T600: production queue/storage with a paced synthetic loopback consumer.
//! Excludes full daemon, capture, encoder, Android, and control-plane tasks.
#![allow(dead_code)]
mod media_storage;
mod video_queue;
mod media {
    #[derive(Clone)]
    pub struct VideoPacket {
        pub data: crate::media_storage::MediaBytes,
        pub is_idr: bool,
        pub seq: u32,
        pub codec_config: Option<crate::media_storage::MediaBytes>,
        pub generation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
}
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

fn usage() -> libc::rusage {
    let mut result = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut result) }, 0);
    result
}

fn micros(time: libc::timeval) -> i64 { time.tv_sec * 1_000_000 + time.tv_usec }

fn receiver(listener: std::net::TcpListener, start: Instant) -> Vec<u64> {
    let (mut socket, _) = listener.accept().unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut bytes = vec![0; 65536];
    let mut rows = Vec::new();
    for index in 0u32..360 {
        socket.read_exact(&mut bytes).unwrap();
        assert_eq!(u32::from_be_bytes(bytes[..4].try_into().unwrap()), index);
        assert!(bytes[12..].iter().all(|byte| *byte == 0x5a));
        let sent = u64::from_be_bytes(bytes[4..12].try_into().unwrap());
        if (60..300).contains(&index) { rows.push(start.elapsed().as_nanos() as u64 - sent); }
    }
    rows
}

fn producer(sender: video_queue::VideoSender, start: Instant) {
    let generation = Arc::new(std::sync::atomic::AtomicBool::new(true));
    for index in 0u32..360 {
        let target = start + Duration::from_nanos(index as u64 * 1_000_000_000 / 60);
        std::thread::sleep(target.saturating_duration_since(Instant::now()));
        let mut bytes = vec![0x5a; 65536];
        bytes[..4].copy_from_slice(&index.to_be_bytes());
        bytes[4..12].copy_from_slice(&(start.elapsed().as_nanos() as u64).to_be_bytes());
        let packet = media::VideoPacket { data: bytes.into(), seq:index, is_idr:true,
            codec_config:None, generation:generation.clone() };
        assert!(sender.send(packet).is_ok());
    }
}

async fn transfer(address: std::net::SocketAddr, mut rx: tokio::sync::broadcast::Receiver<media::VideoPacket>) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket.set_nodelay(true).unwrap();
    for _ in 0..360 {
        let packet = rx.recv().await.unwrap();
        socket.write_all(&packet.data).await.unwrap();
    }
}

fn main() {
    let workers: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    assert!((1..=32).contains(&workers));
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(workers).enable_all().build().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, rx) = video_queue::channel(8, Default::default());
    let before = usage();
    let start = Instant::now() + Duration::from_millis(50);
    let reader = std::thread::spawn(move || receiver(listener, start));
    let producer = std::thread::spawn(move || producer(sender, start));
    runtime.block_on(async { tokio::spawn(transfer(address, rx)).await.unwrap(); });
    producer.join().unwrap();
    let rows = reader.join().unwrap();
    let after = usage();
    println!("{}", serde_json::json!({"workers":workers, "latency_ns":rows,
        "cpu_us":micros(after.ru_utime)+micros(after.ru_stime)-micros(before.ru_utime)-micros(before.ru_stime),
        "voluntary_switches":after.ru_nvcsw-before.ru_nvcsw,
        "involuntary_switches":after.ru_nivcsw-before.ru_nivcsw,
        "rss_peak_kib":after.ru_maxrss,"frames":360,"measured_frames":240}));
}
