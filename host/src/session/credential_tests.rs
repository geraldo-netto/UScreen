//! T444: real loopback admission, without capture, ADB or input devices.
use super::*;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message};

struct Fixture {
    tablet: crate::attachment::Attachment,
    video: std::net::SocketAddr,
    input: std::net::SocketAddr,
    tasks: Vec<JoinHandle<()>>,
}
impl Fixture {
    async fn new() -> Self {
        let (mode, _) = watch::channel(true);
        let prepared = Spec {
            capture: capture::CaptureConfig::default(),
            ports: (0, 0),
            token: Some("a".repeat(64)),
            devices: (false, false, false),
        }
        .prepare(mode);
        prepared
            .resources
            .codec_config
            .publish(Some(crate::media_storage::MediaBytes::from_static(b"csd")));
        let video_listener = prepared.stream.bind().await.unwrap();
        let input_listener = prepared.input.bind().await.unwrap();
        let video = video_listener.local_addr().unwrap();
        let input = input_listener.local_addr().unwrap();
        let (sender, _) = crate::video_queue::channel(8, Default::default());
        let tasks = vec![
            tokio::spawn(async move {
                prepared
                    .stream
                    .run_with_listener(sender, video_listener)
                    .await
                    .unwrap();
            }),
            tokio::spawn(async move {
                prepared
                    .input
                    .run_with_listener(input_listener)
                    .await
                    .unwrap();
            }),
        ];
        Self {
            tablet: prepared.tablet,
            video,
            input,
            tasks,
        }
    }
    async fn video(&self, token: &str) -> TcpStream {
        let mut socket = TcpStream::connect(self.video).await.unwrap();
        socket.write_all(token.as_bytes()).await.unwrap();
        socket
    }
    async fn stop(self) {
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

async fn config(socket: &mut TcpStream) {
    let mut bytes = [0; 8];
    tokio::time::timeout(Duration::from_secs(1), socket.read_exact(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes[5..], b"csd");
}
async fn closed(socket: &mut TcpStream) {
    let mut byte = [0];
    let result = tokio::time::timeout(Duration::from_secs(1), socket.read(&mut byte)).await;
    assert!(
        matches!(result, Ok(Ok(0)) | Ok(Err(_))),
        "T444: retired video route still admitted: {result:?}"
    );
}

#[tokio::test]
async fn t444_late_control_cannot_use_replaced_tablet_token() {
    let f = Fixture::new().await;
    f.tablet.begin(Some("replacement".into()));
    let (mut client, _) = connect_async(format!("ws://{}", f.input)).await.unwrap();
    client
        .send(Message::Text(
            serde_json::json!({"type":"auth", "token":"a".repeat(64)}).to_string(),
        ))
        .await
        .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(1), client.next())
        .await
        .unwrap();
    assert!(
        !matches!(reply, Some(Ok(Message::Text(_)))),
        "T444: retired control token received greeting: {reply:?}"
    );
    f.stop().await;
}

#[tokio::test]
async fn t444_late_video_cannot_use_replaced_tablet_token() {
    let f = Fixture::new().await;
    f.tablet.begin(Some("replacement".into()));
    closed(&mut f.video(&"a".repeat(64)).await).await;
    f.stop().await;
}

#[tokio::test]
async fn t444_replacement_retires_already_authenticated_video() {
    let f = Fixture::new().await;
    let mut socket = f.video(&"a".repeat(64)).await;
    config(&mut socket).await;
    f.tablet.begin(Some("replacement".into()));
    closed(&mut socket).await;
    f.stop().await;
}

#[tokio::test]
async fn t444_current_credential_reconnects_and_same_identity_migrates() {
    let f = Fixture::new().await;
    f.tablet.begin(Some("physical-A".into()));
    let token = f.tablet.token().unwrap().unwrap();
    assert_ne!(token, "a".repeat(64));
    assert_eq!(token.len(), 64);
    let mut first = f.video(&token).await;
    config(&mut first).await;
    drop(first);
    let mut second = f.video(&token).await;
    config(&mut second).await;
    let (mut client, _) = connect_async(format!("ws://{}", f.input)).await.unwrap();
    client
        .send(Message::Text(
            serde_json::json!({"type":"auth", "token":token}).to_string(),
        ))
        .await
        .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(1), client.next())
        .await
        .unwrap();
    assert!(
        matches!(reply, Some(Ok(Message::Text(_)))),
        "current control credential rejected"
    );
    f.tablet.begin_with_transport(
        Some("physical-A".into()),
        Some(blent_config::adb::Transport::Network),
    );
    assert_eq!(f.tablet.token().unwrap().as_deref(), Some(token.as_str()));
    closed(&mut second).await;
    config(&mut f.video(&token).await).await;
    let _ = f.tablet.send(false);
    f.tablet.begin(Some("physical-A".into()));
    assert_ne!(f.tablet.token().unwrap().unwrap(), token);
    closed(&mut f.video(&token).await).await;
    f.stop().await;
}

#[tokio::test]
async fn t444_invalid_and_out_of_bounds_auth_corpus_is_rejected() {
    let f = Fixture::new().await;
    let mut corpus = vec![
        vec![],
        vec![b'a'],
        vec![b'a'; 63],
        vec![0xff; 64],
        vec![b'b'; 65],
    ];
    for i in 0..64 {
        let mut token = vec![b'a'; 64];
        token[i] ^= 0x80;
        corpus.push(token);
    }
    for token in corpus {
        let mut socket = TcpStream::connect(f.video).await.unwrap();
        socket.write_all(&token).await.unwrap();
        socket.shutdown().await.unwrap();
        closed(&mut socket).await;
    }
    for token in [
        serde_json::Value::Null,
        serde_json::json!(-1),
        serde_json::json!([]),
        serde_json::json!(""),
        serde_json::json!("a".repeat(63)),
        serde_json::json!("a".repeat(65)),
        serde_json::json!("a".repeat(65_536)),
    ] {
        let (mut client, _) = connect_async(format!("ws://{}", f.input)).await.unwrap();
        let _ = client
            .send(Message::Text(
                serde_json::json!({"type":"auth", "token":token}).to_string(),
            ))
            .await;
        let reply = tokio::time::timeout(Duration::from_secs(1), client.next())
            .await
            .unwrap();
        assert!(
            !matches!(reply, Some(Ok(Message::Text(_)))),
            "T444: malformed auth admitted"
        );
    }
    f.stop().await;
}

#[tokio::test]
async fn t444_stalled_accepted_work_never_polls_after_replacement() {
    let f = Fixture::new().await;
    let lease = f.tablet.lease();
    f.tablet.begin(Some("replacement".into()));
    lease
        .run(async { panic!("T444: retired accepted I/O was polled") })
        .await
        .unwrap();
    f.stop().await;
}

#[tokio::test]
async fn t444_slots_with_same_bootstrap_seed_do_not_share_credentials() {
    let first = Fixture::new().await;
    let second = Fixture::new().await;
    first.tablet.begin(Some("physical-A".into()));
    second.tablet.begin(Some("physical-B".into()));
    let a = first.tablet.token().unwrap().unwrap();
    let b = second.tablet.token().unwrap().unwrap();
    assert_ne!(a, b);
    config(&mut first.video(&a).await).await;
    config(&mut second.video(&b).await).await;
    closed(&mut first.video(&b).await).await;
    closed(&mut second.video(&a).await).await;
    first.stop().await;
    second.stop().await;
}
