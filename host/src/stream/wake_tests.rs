//! T405: wake on actual state changes, retaining the startup deadline.
use super::*;
use crate::poll_probe::Probe;

#[tokio::test(start_paused = true)]
async fn t405_idle_listener_has_no_periodic_wakeups() {
    let server = StreamServer::new(Default::default(), Default::default(), Default::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (tx, _rx) = crate::video_queue::channel(8, Default::default());
    let mut future = Box::pin(server.run_with_listener(tx, listener));
    let probe = Arc::new(Probe::default());
    assert!(probe.poll(future.as_mut()).is_pending());
    probe.take();
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    assert_eq!(probe.take(), 0, "T405: idle video listener woke on a timer");
}

#[tokio::test(start_paused = true)]
async fn t405_stop_wakes_listener_without_waiting_for_a_tick() {
    let server = StreamServer::new(Default::default(), Default::default(), Default::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (tx, _rx) = crate::video_queue::channel(8, Default::default());
    let mut future = Box::pin(server.run_with_listener(tx, listener));
    let probe = Arc::new(Probe::default());
    assert!(probe.poll(future.as_mut()).is_pending());
    probe.take();
    server.stop();
    assert!(probe.take() > 0, "T405: stop left the listener asleep");
    assert!(probe.poll(future.as_mut()).is_ready());
}

async fn playback() -> (ClientPlayback, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let viewer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (server, _) = listener.accept().await.unwrap();
    let (_, socket) = server.into_split();
    (
        ClientPlayback {
            socket,
            last_sent_config: None,
            last_generation: None,
            wait_for_idr: true,
            dropped: 0,
            scratch: VecDeque::new(),
        },
        viewer,
    )
}

#[tokio::test(start_paused = true)]
async fn t405_codec_config_publication_wakes_waiters_immediately() {
    let (mut client, mut viewer) = playback().await;
    let config = CodecConfig::default();
    let budget = crate::media_storage::Budget::new(1024);
    let mut future = Box::pin(client.send_initial_config(&config, &budget));
    let probe = Arc::new(Probe::default());
    assert!(probe.poll(future.as_mut()).is_pending());
    probe.take();
    config.publish(Some(Bytes::from_static(b"csd")));
    assert!(
        probe.take() > 0,
        "T405: CSD publication waited for a poll tick"
    );
    future.await.unwrap();
    use tokio::io::AsyncReadExt;
    let mut bytes = [0; 8];
    viewer.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes[5..], b"csd");
}

#[tokio::test(start_paused = true)]
async fn t405_codec_config_wait_has_only_its_final_deadline() {
    let (mut client, _viewer) = playback().await;
    let config = CodecConfig::default();
    let budget = crate::media_storage::Budget::new(1024);
    let mut future = Box::pin(client.send_initial_config(&config, &budget));
    let probe = Arc::new(Probe::default());
    assert!(probe.poll(future.as_mut()).is_pending());
    probe.take();
    tokio::time::advance(std::time::Duration::from_secs(4)).await;
    assert_eq!(probe.take(), 0, "T405: missing CSD caused periodic wakeups");
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    assert!(
        probe.poll(future.as_mut()).is_ready(),
        "T405: original five-second deadline was lost"
    );
}

#[tokio::test(start_paused = true)]
async fn t405_stop_before_first_poll_remains_latched() {
    let server = StreamServer::new(Default::default(), Default::default(), Default::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (tx, _rx) = crate::video_queue::channel(8, Default::default());
    server.stop();
    let mut future = Box::pin(server.run_with_listener(tx, listener));
    let probe = Arc::new(Probe::default());
    assert!(
        probe.poll(future.as_mut()).is_ready(),
        "T405: startup erased the stop request"
    );
}
