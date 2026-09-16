use crate::capture::VideoPacket;
use anyhow::Result;
use bytes::Bytes;
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
        video_tx: broadcast::Sender<VideoPacket>,
        listener: TcpListener,
    ) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        // Dropping this server future also cancels every accepted connection.
        let mut clients = tokio::task::JoinSet::new();

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

            info!("Client connected: {}", peer);
            // The optional in-process encoder honors this next-frame request;
            // the CLI path supplies periodic wall-clock IDRs.
            self.idr_wanted.store(true, Ordering::SeqCst);
            let rx = video_tx.subscribe();
            let cc = self.codec_config.clone();
            let token = self.config.token.clone();
            clients.spawn(async move {
                if let Err(e) = Self::handle_client(socket, rx, cc, token).await {
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
        mut rx: broadcast::Receiver<VideoPacket>,
        codec_config: Arc<Mutex<Option<Bytes>>>,
        token: Option<String>,
    ) -> Result<()> {
        // Disable Nagle's algorithm for lower latency
        socket.set_nodelay(true)?;

        // Nothing leaves this socket until the client has proved it is the
        // tablet we launched. The first 64 bytes are the session token in
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

        let mut last_sent_config: Option<Bytes> = None;

        // Send cached codec config (SPS/PPS) so MediaCodec can configure.
        // If not yet available, wait briefly for it.
        let mut retries = 0;
        loop {
            let codec_data: Option<Bytes> = codec_config.lock().ok().and_then(|g| g.clone());
            if let Some(config) = codec_data {
                info!("Sending codec config to client ({} bytes)", config.len());
                Self::write_packet(&mut socket, PACKET_TYPE_CONFIG, &config).await?;
                last_sent_config = Some(config);
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

        // New clients can only start decoding at an IDR
        let mut wait_for_idr = true;
        let mut dropped: u64 = 0;

        loop {
            let first = match rx.recv().await {
                Ok(d) => d,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Client lagged {} frames, resuming at next IDR", n);
                    wait_for_idr = true;
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };

            // Drain whatever else is already queued so we can see how far
            // behind this client is.
            let mut batch = vec![first];
            loop {
                match rx.try_recv() {
                    Ok(p) => batch.push(p),
                    Err(broadcast::error::TryRecvError::Lagged(n)) => {
                        warn!("Client lagged {} frames, resuming at next IDR", n);
                        wait_for_idr = true;
                        batch.clear();
                    }
                    Err(_) => break,
                }
            }

            // Too far behind: jump to the freshest IDR if one is queued.
            // Frames before an IDR are never needed to decode what follows it.
            if batch.len() > MAX_BACKLOG {
                if let Some(pos) = batch.iter().rposition(|p| p.is_idr) {
                    dropped += pos as u64;
                    batch.drain(..pos);
                }
            }

            let current_config = codec_config.lock().ok().and_then(|g| g.clone());
            if let Some(config) = current_config {
                if last_sent_config.as_ref() != Some(&config) {
                    info!(
                        "Sending refreshed codec config to client ({} bytes)",
                        config.len()
                    );
                    Self::write_packet(&mut socket, PACKET_TYPE_CONFIG, &config).await?;
                    last_sent_config = Some(config);
                    wait_for_idr = true;
                }
            }

            for packet in batch {
                if wait_for_idr {
                    if !packet.is_idr {
                        dropped += 1;
                        continue;
                    }
                    if dropped > 0 {
                        info!("Resumed at IDR after dropping {} frames", dropped);
                        dropped = 0;
                    }
                    wait_for_idr = false;
                }
                Self::write_frame(&mut socket, packet.seq, &packet.data).await?;
            }
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

    async fn write_packet(socket: &mut TcpStream, packet_type: u8, payload: &[u8]) -> Result<()> {
        let packet_len = payload.len() + 1;
        let len_buf = (packet_len as u32).to_be_bytes();
        socket.write_all(&len_buf).await?;
        socket.write_all(&[packet_type]).await?;
        socket.write_all(payload).await?;
        Ok(())
    }

    /// Frame packets carry a 4-byte big-endian sequence number after the type
    /// byte. The tablet hands it to the decoder as the presentation timestamp
    /// and echoes it back once the frame is on screen, which is what makes
    /// end-to-end latency measurable on a single clock.
    async fn write_frame(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::IoSlice,
        pin::Pin,
        task::{Context, Poll},
    };

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
            let (tx, _) = broadcast::channel(8);
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
        let (tx, _) = broadcast::channel(8);
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
