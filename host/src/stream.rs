#[cfg(test)]
mod resources;

use crate::media::VideoPacket;
use crate::media_storage::MediaBytes as Bytes;
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

const PACKET_TYPE_CONFIG: u8 = 0;
const PACKET_TYPE_FRAME: u8 = 1;

/// If more than this many frames are queued for a client, skip ahead to the
/// most recent IDR instead of letting latency accumulate.
const MAX_BACKLOG: usize = 2;

/// Kernel send-buffer cap, in bytes. Roughly a couple of frames' worth at the
/// rates this streams at — enough to absorb scheduling jitter, too small to
/// hide a genuinely slow link.
const SEND_BUFFER_BYTES: libc::c_int = 128 * 1024;

pub struct StreamConfig {
    pub video_port: u16,
    /// This run's session token; `None` disables the check.
    pub token: Option<String>,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            video_port: 8890,
            token: None,
        }
    }
}

/// How long a client gets to present the token before the socket is closed.
const AUTH_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(3);
const MAX_CLIENTS: usize = 16;
const WRITE_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(1);

pub struct StreamServer {
    config: StreamConfig,
    running: Arc<AtomicBool>,
    codec_config: Arc<Mutex<Option<Bytes>>>,
    /// Raised on attachment for the optional in-process encoder's next-frame
    /// keyframe request. The CLI uses its one-second wall-clock IDR schedule.
    idr_wanted: Arc<AtomicBool>,
}

impl StreamServer {
    pub fn new(
        config: StreamConfig,
        codec_config: Arc<Mutex<Option<Bytes>>>,
        idr_wanted: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            codec_config,
            idr_wanted,
        }
    }

    pub async fn bind(&self) -> Result<TcpListener> {
        // The tablet reaches loopback through adb reverse.
        let addr = format!("127.0.0.1:{}", self.config.video_port);
        let listener = TcpListener::bind(&addr).await?;
        info!("Stream server on tcp://{}", addr);
        Ok(listener)
    }

    pub async fn run_with_listener(
        &self,
        video_tx: crate::video_queue::VideoSender,
        listener: TcpListener,
    ) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        // Dropping this server future also cancels every accepted connection.
        let mut clients = tokio::task::JoinSet::new();
        let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CLIENTS));

        loop {
            let accept = tokio::select! {
                res = listener.accept() => res,
                _ = clients.join_next(), if !clients.is_empty() => continue,
                _ = async {
                    while running.load(Ordering::SeqCst) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                } => break,
            };

            let (socket, peer) = match accept {
                Ok(s) => s,
                Err(e) => {
                    error!("Accept failed: {}", e);
                    continue;
                }
            };

            let Ok(permit) = slots.clone().try_acquire_owned() else {
                warn!("Video client limit reached — dropping {}", peer);
                continue;
            };
            info!("Client connected: {}", peer);
            let tx = video_tx.clone();
            let idr_wanted = self.idr_wanted.clone();
            let cc = self.codec_config.clone();
            let token = self.config.token.clone();
            clients.spawn(async move {
                let _permit = permit;
                if let Err(e) = Self::handle_client(socket, tx, cc, token, idr_wanted).await {
                    warn!("Client {} disconnected: {}", peer, e);
                }
                info!("Client {} session ended", peer);
            });
        }

        clients.shutdown().await;
        Ok(())
    }

    async fn handle_client(
        mut socket: TcpStream,
        video_tx: crate::video_queue::VideoSender,
        codec_config: Arc<Mutex<Option<Bytes>>>,
        token: Option<String>,
        idr_wanted: Arc<AtomicBool>,
    ) -> Result<()> {
        // Disable Nagle's algorithm for lower latency
        socket.set_nodelay(true)?;

        // With authentication enabled, nothing leaves this socket until the
        // client has proved it is the tablet we launched. The first 64 bytes are the session token in
        // hex; anything else, or silence, and the connection is dropped.
        if let Some(expected) = token.as_deref() {
            use tokio::io::AsyncReadExt;
            let mut buf = [0u8; 64];
            let read = tokio::time::timeout(AUTH_TIMEOUT, socket.read_exact(&mut buf)).await;
            let ok = matches!(read, Ok(Ok(_)))
                && std::str::from_utf8(&buf)
                    .map(|t| crate::runtime::token_matches(expected, t))
                    .unwrap_or(false);
            if !ok {
                warn!(
                    "Video client did not present a valid session token — dropped. \
                       An app older than 1.1.0 cannot authenticate: update it."
                );
                return Ok(());
            }
        }

        // Cap the kernel send buffer. Linux auto-tunes this into the megabytes,
        // which on a link slower than the encoder means frames sit invisibly in
        // the socket instead of surfacing as backpressure — the skip-ahead
        // logic below never sees them, and the delay lands on screen instead.
        // A small buffer makes a slow link show up immediately as a blocked
        // write, which is exactly what the backlog handling needs to react to.
        Self::set_send_buffer(&socket, SEND_BUFFER_BYTES);

        // Only admitted peers count as viewers or request encoder work.
        let rx = video_tx.subscribe();
        idr_wanted.store(true, Ordering::SeqCst);
        let (mut reader, writer) = socket.into_split();
        tokio::select! {
            result = Self::stream_packets(writer, rx, codec_config, video_tx.budget()) => result,
            // Disabled authentication accepts the Android client's saved token
            // as an optional prelude, without delaying tokenless legacy clients.
            // EOF, malformed/extra input and incomplete preludes end the viewer.
            _ = watch_client_input(&mut reader, token.is_none()) => Ok(()),
        }
    }

    async fn stream_packets(
        socket: tokio::net::tcp::OwnedWriteHalf,
        mut rx: broadcast::Receiver<VideoPacket>,
        codec_config: Arc<Mutex<Option<Bytes>>>,
        budget: Arc<crate::media_storage::Budget>,
    ) -> Result<()> {
        let mut client = ClientPlayback {
            socket,
            last_sent_config: None,
            last_generation: None,
            wait_for_idr: true,
            dropped: 0,
            scratch: VecDeque::with_capacity(crate::video_queue::QUEUE_PACKETS),
        };
        client.send_initial_config(&codec_config, &budget).await?;
        loop {
            let first = match rx.recv().await {
                Ok(d) => d,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Client lagged {} frames, resuming at next IDR", n);
                    client.wait_for_idr = true;
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
            let batch = client.drain_batch(&mut rx, first);
            // Retained batch lifetime is bounded too, including a client
            // that makes tiny progress before each individual write deadline.
            tokio::time::timeout(WRITE_TIMEOUT, client.send_batch(batch)).await??;
        }
        Ok(())
    }

    /// Best-effort: a kernel that refuses the hint is not a reason to fail the
    /// connection, it just means latency behaves as it did before.
    fn set_send_buffer(socket: &TcpStream, bytes: libc::c_int) {
        use std::os::fd::AsRawFd;
        let fd = socket.as_raw_fd();
        let rc = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &bytes as *const libc::c_int as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            warn!(
                "Could not set SO_SNDBUF: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    async fn write_packet(
        socket: &mut (impl tokio::io::AsyncWrite + Unpin),
        packet_type: u8,
        payload: &[u8],
    ) -> Result<()> {
        tokio::time::timeout(
            WRITE_TIMEOUT,
            Self::write_packet_bytes(socket, packet_type, payload),
        )
        .await?
    }

    async fn write_packet_bytes(
        socket: &mut (impl tokio::io::AsyncWrite + Unpin),
        packet_type: u8,
        payload: &[u8],
    ) -> Result<()> {
        let packet_len = payload.len() + 1;
        let len_buf = (packet_len as u32).to_be_bytes();
        socket.write_all(&len_buf).await?;
        socket.write_all(&[packet_type]).await?;
        socket.write_all(payload).await?;
        Ok(())
    }

    /// Frame packets carry a 4-byte big-endian sequence number after the type
    /// byte. The tablet hands it to the decoder as the presentation timestamp
    /// and echoes it back once the frame is on screen. This measures send-to-ack
    /// latency on one clock; capture and encoding occur before that interval.
    async fn write_frame(
        socket: &mut (impl tokio::io::AsyncWrite + Unpin),
        seq: u32,
        payload: &[u8],
    ) -> Result<()> {
        tokio::time::timeout(WRITE_TIMEOUT, Self::write_frame_bytes(socket, seq, payload)).await?
    }

    async fn write_frame_bytes(
        socket: &mut (impl tokio::io::AsyncWrite + Unpin),
        seq: u32,
        payload: &[u8],
    ) -> Result<()> {
        let mut header = [0u8; 9];
        header[..4].copy_from_slice(&((payload.len() + 5) as u32).to_be_bytes());
        header[4] = PACKET_TYPE_FRAME;
        header[5..].copy_from_slice(&seq.to_be_bytes());
        let mut slices = [
            std::io::IoSlice::new(&header),
            std::io::IoSlice::new(payload),
        ];
        let mut remaining = &mut slices[..];
        while !remaining.is_empty() {
            let written = socket.write_vectored(remaining).await?;
            if written == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into());
            }
            std::io::IoSlice::advance_slices(&mut remaining, written);
        }
        Ok(())
    }

    /// Counterpart to `run`; shutdown currently goes through task
    /// cancellation instead, but leaving this makes the lifecycle explicit.
    #[allow(dead_code)]
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Monitor disconnects while streaming. With checks disabled, consume at most
/// one optional 64-byte hex token; after its first byte the usual auth deadline
/// bounds completion. This never participates in enabled authentication.
async fn watch_client_input(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    optional_token: bool,
) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let first = reader.read_u8().await?;
    if !optional_token || !first.is_ascii_hexdigit() {
        return Ok(());
    }
    let mut rest = [0u8; 63];
    tokio::time::timeout(AUTH_TIMEOUT, reader.read_exact(&mut rest)).await??;
    if !rest.iter().all(u8::is_ascii_hexdigit) {
        return Ok(());
    }
    // No more client input belongs on the video socket after this prelude.
    let _ = reader.read_u8().await?;
    Ok(())
}

/// Decode readiness and backlog state owned by one admitted viewer.
struct ClientPlayback {
    socket: tokio::net::tcp::OwnedWriteHalf,
    last_sent_config: Option<Bytes>,
    last_generation: Option<Arc<AtomicBool>>,
    wait_for_idr: bool,
    dropped: u64,
    scratch: VecDeque<VideoPacket>,
}

impl ClientPlayback {
    async fn send_initial_config(
        &mut self,
        codec_config: &Mutex<Option<Bytes>>,
        budget: &Arc<crate::media_storage::Budget>,
    ) -> Result<()> {
        // Send cached codec config (SPS/PPS) so MediaCodec can configure.
        // If not yet available, wait briefly for it.
        let mut retries = 0;
        loop {
            let codec_data: Option<Bytes> = cached_codec_config(codec_config);
            if let Some(config) = codec_data {
                anyhow::ensure!(
                    config.len() <= crate::video_queue::MAX_CONFIG_BYTES && config.charge(budget),
                    "Initial codec configuration exceeds encoded storage or packet limit"
                );
                info!("Sending codec config to client ({} bytes)", config.len());
                StreamServer::write_packet(&mut self.socket, PACKET_TYPE_CONFIG, &config).await?;
                self.last_sent_config = Some(config);
                break;
            }
            retries += 1;
            if retries > 50 {
                // 5 seconds
                warn!("Codec config not available after 5s, starting stream without it");
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        Ok(())
    }

    fn drain_batch(
        &mut self,
        rx: &mut broadcast::Receiver<VideoPacket>,
        first: VideoPacket,
    ) -> VecDeque<VideoPacket> {
        // Drain whatever else is already queued so we can see how far
        // behind this client is.
        let mut batch = std::mem::take(&mut self.scratch);
        batch.push_back(first);
        // A continuously producing encoder must not extend one drain forever.
        for _ in 1..crate::video_queue::QUEUE_PACKETS {
            match rx.try_recv() {
                Ok(p) => batch.push_back(p),
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    warn!("Client lagged {} frames, resuming at next IDR", n);
                    self.wait_for_idr = true;
                    batch.clear();
                }
                Err(_) => break,
            }
        }

        // Too far behind: jump to the freshest IDR if one is queued.
        // Frames before an IDR are never needed to decode what follows it.
        if batch.len() > MAX_BACKLOG {
            if let Some(pos) = batch.iter().rposition(|p| p.is_idr) {
                self.dropped += pos as u64;
                for _ in 0..pos {
                    batch.pop_front();
                }
            }
        }

        batch
    }

    async fn prepare_packet(&mut self, packet: &VideoPacket) -> Result<bool> {
        if !packet.generation.load(Ordering::Acquire) {
            self.wait_for_idr = true;
            return Ok(false);
        }
        let Some(config) = packet.codec_config.as_ref() else {
            return Ok(false);
        };
        let same_generation = self
            .last_generation
            .as_ref()
            .is_some_and(|previous| Arc::ptr_eq(previous, &packet.generation));
        if !same_generation {
            self.wait_for_idr = true;
            self.last_generation = Some(packet.generation.clone());
        }
        if self.last_sent_config.as_ref() != Some(config) {
            StreamServer::write_packet(&mut self.socket, PACKET_TYPE_CONFIG, config).await?;
            self.last_sent_config = Some(config.clone());
            self.wait_for_idr = true;
        }
        // Configuration writes may block while the encoder is retired.
        Ok(packet.generation.load(Ordering::Acquire))
    }

    async fn send_batch(&mut self, mut batch: VecDeque<VideoPacket>) -> Result<()> {
        while let Some(packet) = batch.pop_front() {
            if !self.prepare_packet(&packet).await? {
                self.dropped += 1;
                continue;
            }
            if self.wait_for_idr {
                if !packet.is_idr {
                    self.dropped += 1;
                    continue;
                }
                if self.dropped > 0 {
                    info!("Resumed at IDR after dropping {} frames", self.dropped);
                    self.dropped = 0;
                }
                self.wait_for_idr = false;
            }
            StreamServer::write_frame(&mut self.socket, packet.seq, &packet.data).await?;
        }
        self.scratch = batch;
        Ok(())
    }
}

fn cached_codec_config(codec_config: &Mutex<Option<Bytes>>) -> Option<Bytes> {
    codec_config.lock().ok().and_then(|g| g.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::IoSlice,
        pin::Pin,
        task::{Context, Poll},
    };

    #[tokio::test]
    async fn t391_initial_config_counts_backing_before_first_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut viewer = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let mut storage = Vec::with_capacity(8192);
        storage.push(1);
        let cache = Arc::new(Mutex::new(Some(Bytes::from(storage))));
        let (tx, _) = crate::video_queue::channel(8, Default::default());
        let task = tokio::spawn(StreamServer::handle_client(
            socket,
            tx.clone(),
            cache.clone(),
            None,
            Default::default(),
        ));
        assert_eq!(
            t227_read_packet(&mut viewer).await,
            (PACKET_TYPE_CONFIG, vec![1])
        );
        let retained = tx.usage().0;
        task.abort();
        let _ = task.await;
        drop(cache);
        assert_eq!(
            retained, 8192,
            "T391: initial codec cache escaped the storage budget"
        );
        assert_eq!(tx.usage().0, 0);
    }

    #[tokio::test]
    async fn t391_stalled_frame_and_config_writes_have_a_deadline() {
        for frame in [false, true] {
            let (mut writer, _unread) = tokio::io::duplex(1);
            let result = tokio::time::timeout(std::time::Duration::from_millis(1250), async {
                if frame {
                    StreamServer::write_frame(&mut writer, 1, b"frame").await
                } else {
                    StreamServer::write_packet(&mut writer, PACKET_TYPE_CONFIG, b"config").await
                }
            })
            .await;
            assert!(
                result.is_ok(),
                "T391: stalled write retained its packet indefinitely"
            );
            assert!(
                result.unwrap().is_err(),
                "T391: incomplete frame reported success"
            );
        }
    }

    #[tokio::test]
    async fn t391_replay_batch_allocations() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let _viewer = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (_, socket) = server.into_split();
        let mut playback = ClientPlayback {
            socket,
            last_sent_config: None,
            last_generation: None,
            wait_for_idr: true,
            dropped: 0,
            scratch: VecDeque::with_capacity(crate::video_queue::QUEUE_PACKETS),
        };
        let generation = crate::media::EncoderGeneration::new();
        let packet = VideoPacket {
            data: Bytes::from(vec![1; 64 * 1024]),
            is_idr: false,
            seq: 0,
            codec_config: Some(Bytes::from_static(b"config")),
            generation: generation.active.clone(),
        };
        let (tx, mut rx) = crate::video_queue::channel(8, Default::default());
        let (_, counts) = crate::allocation_probe::measure(|| {
            for _ in 0..10000 {
                for _ in 0..8 {
                    tx.send(packet.clone()).ok().unwrap();
                }
                let first = rx.try_recv().unwrap();
                let batch = playback.drain_batch(&mut rx, first);
                assert_eq!(batch.len(), 8);
                let mut batch = batch;
                batch.clear();
                playback.scratch = batch;
            }
        });
        assert!(
            counts.allocations <= 1 && counts.reallocations == 0,
            "T391: batch allocation returned"
        );
        println!(
            "T391 batches=10000 allocations={} reallocations={} requested_bytes={}",
            counts.allocations, counts.reallocations, counts.requested_bytes
        );
    }

    async fn t227_read_packet(socket: &mut TcpStream) -> (u8, Vec<u8>) {
        use tokio::io::AsyncReadExt;
        let len = socket.read_u32().await.unwrap();
        let kind = socket.read_u8().await.unwrap();
        let mut data = vec![0; len as usize - 1];
        socket.read_exact(&mut data).await.unwrap();
        (kind, data)
    }

    #[tokio::test]
    async fn t227_queued_frame_keeps_its_codec_configuration() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut viewer = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (_, writer) = server.into_split();
        let old = Bytes::from_static(b"h264-1280x800");
        let new = Bytes::from_static(b"hevc-1920x1080");
        let (tx, rx) = crate::video_queue::channel(8, Default::default());
        tx.send(VideoPacket {
            seq: 0,
            is_idr: true,
            data: old.clone(),
            codec_config: Some(old.clone()),
            generation: Arc::new(AtomicBool::new(true)),
        })
        .ok()
        .unwrap();
        // The cache changes after encoding/queueing, before this client runs.
        let playback = tokio::spawn(StreamServer::stream_packets(
            writer,
            rx,
            Arc::new(Mutex::new(Some(new.clone()))),
            tx.budget(),
        ));
        assert_eq!(
            t227_read_packet(&mut viewer).await,
            (PACKET_TYPE_CONFIG, new.to_vec())
        );
        assert_eq!(
            t227_read_packet(&mut viewer).await,
            (PACKET_TYPE_CONFIG, old.to_vec()),
            "T227: each frame must follow its own encoder's configuration"
        );
        let (kind, frame) = t227_read_packet(&mut viewer).await;
        assert_eq!(kind, PACKET_TYPE_FRAME);
        assert_eq!(&frame[4..], old.as_ref());
        drop(tx);
        playback.await.unwrap().unwrap();
    }

    async fn t227_retirement_case(new: &'static [u8]) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut viewer = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (_, socket) = server.into_split();
        let mut playback = ClientPlayback {
            socket,
            last_sent_config: None,
            last_generation: None,
            wait_for_idr: true,
            dropped: 0,
            scratch: VecDeque::with_capacity(crate::video_queue::QUEUE_PACKETS),
        };
        let old = crate::media::EncoderGeneration::new();
        let fresh = crate::media::EncoderGeneration::new();
        let packet =
            |seq, is_idr, data: &'static [u8], generation: &crate::media::EncoderGeneration| {
                VideoPacket {
                    seq,
                    is_idr,
                    data: Bytes::from_static(data),
                    codec_config: Some(Bytes::from_static(data)),
                    generation: generation.active.clone(),
                }
            };
        let (tx, mut rx) = crate::video_queue::channel(8, Default::default());
        tx.send(packet(0, true, b"h264-1280x800", &old))
            .ok()
            .unwrap();
        let first = rx.recv().await.unwrap();
        let mut delayed = playback.drain_batch(&mut rx, first);
        drop(old); // restart after drain, while this client was delayed
        delayed.push_back(packet(1, false, new, &fresh));
        delayed.push_back(packet(2, true, new, &fresh));
        playback.send_batch(delayed).await.unwrap();
        drop(playback);
        assert_eq!(
            t227_read_packet(&mut viewer).await,
            (PACKET_TYPE_CONFIG, new.to_vec())
        );
        let (kind, frame) = t227_read_packet(&mut viewer).await;
        assert_eq!(kind, PACKET_TYPE_FRAME);
        assert_eq!(&frame[..4], &2u32.to_be_bytes());
        assert_eq!(&frame[4..], new);
        use tokio::io::AsyncReadExt;
        assert_eq!(
            viewer.read(&mut [0]).await.unwrap(),
            0,
            "T227: retired frames escaped"
        );
    }

    #[tokio::test]
    async fn t227_retired_batches_are_discarded_across_codec_and_resolution_changes() {
        t227_retirement_case(b"h264-1920x1080").await;
        t227_retirement_case(b"hevc-1920x1080").await;
    }

    async fn t267_connection(
        expected: Option<String>,
    ) -> (
        TcpStream,
        crate::video_queue::VideoSender,
        tokio::task::JoinHandle<Result<()>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let viewer = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let (tx, _) = crate::video_queue::channel(8, Default::default());
        let task = tokio::spawn(StreamServer::handle_client(
            socket,
            tx.clone(),
            Arc::new(Mutex::new(Some(Bytes::from_static(b"headers")))),
            expected,
            Default::default(),
        ));
        (viewer, tx, task)
    }

    async fn t267_expect_stream(expected: Option<String>, saved: Option<&str>) {
        use tokio::time::{timeout, Duration};
        let (mut viewer, tx, task) = t267_connection(expected).await;
        if let Some(token) = saved {
            // Android writes the saved token before reading any video. TCP
            // fragmentation must not turn that optional prefix into extra input.
            for chunk in token.as_bytes().chunks(3) {
                viewer.write_all(chunk).await.unwrap();
                tokio::task::yield_now().await;
            }
        }
        assert_eq!(
            timeout(Duration::from_secs(1), t227_read_packet(&mut viewer))
                .await
                .unwrap(),
            (PACKET_TYPE_CONFIG, b"headers".to_vec())
        );
        // Ensure the monitor has consumed the prefix before sending a new IDR.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            tx.receiver_count(),
            1,
            "T267 saved token disconnected a viewer"
        );
        tx.send(VideoPacket {
            seq: 42,
            is_idr: true,
            data: Bytes::from_static(b"frame"),
            codec_config: Some(Bytes::from_static(b"headers")),
            generation: Arc::new(AtomicBool::new(true)),
        })
        .ok()
        .unwrap();
        let (kind, data) = timeout(Duration::from_secs(1), t227_read_packet(&mut viewer))
            .await
            .unwrap();
        assert_eq!(kind, PACKET_TYPE_FRAME);
        assert_eq!(&data[..4], &42u32.to_be_bytes());
        assert_eq!(&data[4..], b"frame");
        drop(viewer);
        timeout(Duration::from_secs(1), task)
            .await
            .expect("T267 idle disconnect leaked capture")
            .unwrap()
            .unwrap();
        assert_eq!(tx.receiver_count(), 0);
    }

    #[tokio::test]
    async fn t267_saved_android_token_survives_enabled_disabled_enabled_transitions() {
        let saved = "a".repeat(64);
        t267_expect_stream(Some(saved.clone()), Some(&saved)).await;
        t267_expect_stream(None, Some(&saved)).await;
        t267_expect_stream(Some(saved.clone()), Some(&saved)).await;
        let rotated = "b".repeat(64);
        t267_expect_stream(Some(rotated.clone()), Some(&rotated)).await;
    }

    #[tokio::test]
    async fn t267_disabled_auth_keeps_legacy_tokenless_video_and_idle_cleanup() {
        t267_expect_stream(None, None).await;
    }

    #[tokio::test]
    async fn t267_enabled_auth_rejects_stale_and_missing_tokens_before_video() {
        use tokio::io::AsyncReadExt;
        for presented in ["a".repeat(64), String::new()] {
            let (mut viewer, tx, task) = t267_connection(Some("b".repeat(64))).await;
            viewer.write_all(presented.as_bytes()).await.unwrap();
            viewer.shutdown().await.unwrap();
            assert_eq!(
                tokio::time::timeout(AUTH_TIMEOUT, viewer.read(&mut [0]))
                    .await
                    .unwrap()
                    .unwrap(),
                0
            );
            task.await.unwrap().unwrap();
            assert_eq!(tx.receiver_count(), 0);
        }
    }

    #[tokio::test]
    async fn t267_disabled_auth_bounds_optional_prefix_and_rejects_extra_input() {
        use tokio::io::AsyncReadExt;
        for prefix in [
            b"x".to_vec(),
            [vec![b'a'; 63], vec![b'z']].concat(),
            [vec![b'a'; 64], vec![b'x']].concat(),
            vec![b'a'],
        ] {
            let (mut viewer, tx, task) = t267_connection(None).await;
            let _ = viewer.write_all(&prefix).await;
            let mut received = Vec::new();
            let closed = tokio::time::timeout(
                AUTH_TIMEOUT + std::time::Duration::from_secs(1),
                viewer.read_to_end(&mut received),
            )
            .await;
            assert!(
                closed.is_ok(),
                "T267 malformed/partial prefix retained viewer"
            );
            // No frames were offered; disabled authentication may send its
            // cached header before the prefix monitor rejects the connection.
            assert!(received.len() <= 12);
            task.await.unwrap().unwrap();
            assert_eq!(tx.receiver_count(), 0);
        }
    }

    async fn authenticated_test_server() -> (
        Arc<StreamServer>,
        crate::video_queue::VideoSender,
        std::net::SocketAddr,
        tokio::task::JoinHandle<Result<()>>,
    ) {
        let server = Arc::new(StreamServer::new(
            StreamConfig {
                token: Some("a".repeat(64)),
                ..Default::default()
            },
            Arc::new(Mutex::new(Some(Bytes::from_static(b"headers")))),
            Default::default(),
        ));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, _) = crate::video_queue::channel(8, Default::default());
        let task = tokio::spawn({
            let server = server.clone();
            let tx = tx.clone();
            async move { server.run_with_listener(tx, listener).await }
        });
        (server, tx, address, task)
    }

    #[tokio::test]
    async fn t139_only_authenticated_clients_request_capture() {
        use tokio::io::AsyncReadExt;
        use tokio::time::{sleep, timeout, Duration};
        let (server, tx, address, task) = authenticated_test_server().await;
        let mut client = TcpStream::connect(address).await.unwrap();
        sleep(Duration::from_millis(20)).await;
        assert_eq!(
            tx.receiver_count(),
            0,
            "pending token subscribed to capture"
        );
        assert!(!server.idr_wanted.load(Ordering::SeqCst));
        client.write_all("b".repeat(64).as_bytes()).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), client.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        assert_eq!(tx.receiver_count(), 0);
        assert!(!server.idr_wanted.load(Ordering::SeqCst));
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all("a".repeat(64).as_bytes()).await.unwrap();
        timeout(Duration::from_secs(1), client.read_exact(&mut [0; 12]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(tx.receiver_count(), 1);
        assert!(server.idr_wanted.load(Ordering::SeqCst));
        server.stop();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn t139_pending_admission_is_bounded_and_reusable() {
        use tokio::io::AsyncReadExt;
        use tokio::time::{sleep, timeout, Duration};
        let (server, _, address, task) = authenticated_test_server().await;
        let mut clients = Vec::new();
        for _ in 0..16 {
            clients.push(TcpStream::connect(address).await.unwrap());
            tokio::task::yield_now().await;
        }
        let mut excess = TcpStream::connect(address).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), excess.read(&mut [0]))
                .await
                .expect("excess unauthenticated connection stayed open")
                .unwrap(),
            0
        );
        drop(clients);
        timeout(Duration::from_secs(1), async {
            loop {
                let mut client = TcpStream::connect(address).await.unwrap();
                if client.write_all("a".repeat(64).as_bytes()).await.is_ok()
                    && client.read_exact(&mut [0; 12]).await.is_ok()
                {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("closed pending clients did not free admission capacity");
        server.stop();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn t139_idle_authenticated_disconnect_frees_capture() {
        use tokio::io::AsyncReadExt;
        use tokio::time::{timeout, Duration};
        let (server, tx, address, task) = authenticated_test_server().await;
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all("a".repeat(64).as_bytes()).await.unwrap();
        timeout(Duration::from_secs(1), client.read_exact(&mut [0; 12]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(tx.receiver_count(), 1);
        drop(client);
        timeout(Duration::from_secs(1), async {
            while tx.receiver_count() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("idle disconnected client retained capture subscription");
        server.stop();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn t138_server_shutdown_retires_authenticated_clients() {
        use tokio::io::AsyncReadExt;
        use tokio::time::{timeout, Duration};
        for abort in [false, true] {
            let token = "a".repeat(64);
            let server = Arc::new(StreamServer::new(
                StreamConfig {
                    token: Some(token.clone()),
                    ..Default::default()
                },
                Arc::new(Mutex::new(Some(Bytes::from_static(b"headers")))),
                Default::default(),
            ));
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let (tx, _) = crate::video_queue::channel(8, Default::default());
            let task = tokio::spawn({
                let server = server.clone();
                let tx = tx.clone();
                async move { server.run_with_listener(tx, listener).await }
            });
            let mut client = TcpStream::connect(address).await.unwrap();
            client.write_all(token.as_bytes()).await.unwrap();
            let mut config = [0; 12];
            timeout(Duration::from_secs(1), client.read_exact(&mut config))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&config[5..], b"headers");
            assert_eq!(tx.receiver_count(), 1);
            if abort {
                task.abort();
            } else {
                server.stop();
            }
            let result = timeout(Duration::from_secs(1), task).await.unwrap();
            if !abort {
                result.unwrap().unwrap();
            }
            let mut byte = [0];
            assert_eq!(
                timeout(Duration::from_secs(1), client.read(&mut byte))
                    .await
                    .expect("server shutdown left a video socket alive")
                    .unwrap(),
                0
            );
            assert_eq!(tx.receiver_count(), 0);
        }
    }

    #[tokio::test]
    async fn t056_idle_server_does_not_count_as_a_video_client() {
        let server = StreamServer::new(
            StreamConfig::default(),
            Default::default(),
            Default::default(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (tx, _) = crate::video_queue::channel(8, Default::default());
        let mut running = Box::pin(server.run_with_listener(tx.clone(), listener));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut running)
                .await
                .is_err()
        );
        assert_eq!(
            tx.receiver_count(),
            0,
            "only connected clients should subscribe"
        );
    }

    #[derive(Default)]
    struct Writer {
        bytes: Vec<u8>,
        writes: usize,
        max_write: usize,
    }
    impl tokio::io::AsyncWrite for Writer {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let n = if self.max_write == 0 {
                bytes.len()
            } else {
                bytes.len().min(self.max_write)
            };
            self.writes += 1;
            self.bytes.extend_from_slice(&bytes[..n]);
            Poll::Ready(Ok(n))
        }
        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            slices: &[IoSlice<'_>],
        ) -> Poll<std::io::Result<usize>> {
            let data: Vec<u8> = slices.iter().flat_map(|s| s.iter().copied()).collect();
            self.as_mut().poll_write(cx, &data)
        }
        fn is_write_vectored(&self) -> bool {
            true
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn t071_frame_header_and_payload_share_one_write_and_handle_short_writes() {
        for max_write in [0, 2, 7, 10] {
            let mut writer = Writer {
                max_write,
                ..Default::default()
            };
            StreamServer::write_frame(&mut writer, 0x12345678, &[9, 8, 7])
                .await
                .unwrap();
            assert_eq!(
                writer.bytes,
                [0, 0, 0, 8, 1, 0x12, 0x34, 0x56, 0x78, 9, 8, 7]
            );
            if max_write == 0 {
                assert_eq!(writer.writes, 1);
            }
        }
    }
}
