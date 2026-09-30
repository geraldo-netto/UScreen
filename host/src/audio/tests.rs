use super::*;
use blent_config::audio::{AudioCapabilities, AudioProfile};
use std::process::Stdio;

async fn sockets() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap());
    let (client, accepted) = tokio::join!(client, listener.accept());
    (client.unwrap(), accepted.unwrap().0)
}
fn grant() -> blent_config::audio::AudioGrant {
    AudioSession::new(Direction::Microphone)
        .start(
            AudioProfile::new(Direction::Microphone),
            AudioCapabilities {
                microphone: true,
                speech: true,
                ..Default::default()
            },
            true,
            0,
        )
        .unwrap()
}
#[tokio::test]
async fn t718_partial_frame_has_one_deadline() {
    let (mut client, mut server) = sockets().await;
    let grant = grant();
    let packet = grant.encode(0, 1, &[0; 480]).unwrap();
    let sender = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(180)).await;
        client.write_all(&packet[..28]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(180)).await;
        let _ = client.write_all(&packet[28..]).await;
    });
    let mut child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(320),
        microphone(&mut server, &mut child, &mut reader),
    )
    .await;
    process::retire(&mut child).await;
    sender.abort();
    assert!(
        matches!(result, Ok(Err(_))),
        "T718 partial packet extended the complete-frame deadline"
    );
}

#[tokio::test]
async fn t718_bootstrap_rejects_bad_peers_and_authenticates_grant() {
    use tokio::io::AsyncReadExt;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let token = "a".repeat(64);
    let request = [b"BLAUREQ1".as_slice(), token.as_bytes(), &[7, 1, 1, 1]].concat();
    let sender = tokio::spawn(async move {
        for index in [0, 8, 72, 73, 74, 75] {
            let mut bad = request.clone();
            bad[index] = 255;
            let mut socket = TcpStream::connect(addr).await.unwrap();
            socket.write_all(&bad).await.unwrap();
        }
        let mut socket = TcpStream::connect(addr).await.unwrap();
        socket.write_all(&request).await.unwrap();
        let mut grant = [0; 156];
        socket.read_exact(&mut grant).await.unwrap();
        grant
    });
    let (mut socket, caps, mode, detail, clocked) =
        protocol::accept(&listener, &token, Direction::Microphone)
            .await
            .unwrap();
    assert!(!clocked);
    assert!(caps.microphone && caps.raw && caps.background);
    assert_eq!(mode, blent_config::audio::Processing::Speech);
    assert!(detail.contains("enabled"));
    let grant = grant();
    protocol::grant(&mut socket, &token, &grant).await.unwrap();
    let bytes = sender.await.unwrap();
    assert_eq!(&bytes[..64], token.as_bytes());
    assert_eq!(&bytes[64..], grant.hello());
}

#[tokio::test(start_paused = true)]
async fn t718_late_request_cannot_extend_consent_deadline() {
    use futures_util::poll;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let token = "a".repeat(64);
    let accepting = protocol::accept(&listener, &token, Direction::Microphone);
    tokio::pin!(accepting);
    assert!(poll!(&mut accepting).is_pending());
    tokio::time::advance(Duration::from_secs(89)).await;
    // A blocking loopback connect does not advance the paused runtime clock.
    let _peer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    tokio::task::yield_now().await;
    assert!(poll!(&mut accepting).is_pending());
    tokio::time::advance(Duration::from_millis(1100)).await;
    assert!(
        matches!(poll!(&mut accepting), std::task::Poll::Ready(Err(_))),
        "T718 partial request extended the 90-second consent deadline"
    );
}

#[tokio::test]
async fn t718_raw_request_reports_effective_capabilities() {
    let token = "a".repeat(64);
    for bits in [2, 3, 6, 7] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let bytes = [b"BLAUREQ1".as_slice(), token.as_bytes(), &[bits, 0, 1, 2]].concat();
        peer.write_all(&bytes).await.unwrap();
        let (_, caps, mode, detail, clocked) =
            protocol::accept(&listener, &token, Direction::Microphone)
                .await
                .unwrap();
        assert!(!clocked);
        assert!(caps.microphone && caps.raw && !caps.speakers);
        assert_eq!(caps.speech, bits & 1 != 0);
        assert_eq!(caps.background, bits & 4 != 0);
        assert_eq!(mode, blent_config::audio::Processing::Raw);
        assert!(detail.contains("AEC unavailable or raw processing selected"));
    }
}

#[tokio::test]
async fn t718_packet_bounds_native_retirement_and_readiness() {
    let grant = grant();
    let packet = grant.encode(0, 1, &[123; 480]).unwrap();
    for length in [0, 1, 27, 28, 500, 987] {
        let mut reader = grant.authenticate(&grant.hello()).unwrap();
        assert!(
            protocol::packet(&mut packet[..length].as_ref(), &mut reader)
                .await
                .is_err()
        );
    }
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    assert_eq!(
        protocol::packet(&mut packet.as_slice(), &mut reader)
            .await
            .unwrap(),
        [&[0][..], &packet[28..]].concat()
    );
    assert!(protocol::packet(&mut packet.as_slice(), &mut reader)
        .await
        .is_err());
    for body in [
        "printf 'READY\\n'; cat >/dev/null",
        "printf 'WRONG!'; cat >/dev/null",
        "sleep 10",
    ] {
        let mut child = Command::new("sh")
            .args(["-c", body])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        if !body.starts_with("sleep") {
            assert_eq!(
                process::ready(&mut child).await.is_ok(),
                body.contains("READY")
            );
        }
        process::retire(&mut child).await;
        assert!(child.try_wait().unwrap().is_some());
    }
    assert!(executable("blent-t718-never-installed").is_err());
    assert!(process::spawn(
        Path::new("/nonexistent/t718"),
        blent_config::audio::AudioProfile::new(Direction::Microphone),
        &"a".repeat(64)
    )
    .is_err());
}

#[tokio::test]
async fn t718_sequence_gap_reaches_native_queue() {
    let grant = grant();
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    let first = grant.encode(0, 1, &[0; 480]).unwrap();
    protocol::packet(&mut first.as_slice(), &mut reader)
        .await
        .unwrap();
    let gap = grant.encode(2, 2, &[123; 480]).unwrap();
    let native = protocol::packet(&mut gap.as_slice(), &mut reader)
        .await
        .unwrap();
    assert_eq!(native[0], 1, "T718 sequence gap must flush native queue");
    assert_eq!(native.len(), 961);
}

#[tokio::test]
async fn t719_speaker_transport_preserves_stereo_and_native_gaps() {
    use tokio::io::AsyncReadExt;
    let (mut client, mut server) = sockets().await;
    let grant = AudioSession::new(Direction::Speakers)
        .start(
            AudioProfile::new(Direction::Speakers),
            AudioCapabilities {
                speakers: true,
                speech: true,
                ..Default::default()
            },
            true,
            0,
        )
        .unwrap();
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    let mut child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for flag in [0, 1] {
        let mut bytes = vec![flag];
        for _ in 0..480 {
            bytes.extend_from_slice(&[0xd2, 0x04, 0x1f, 0xef]);
        }
        input.write_all(&bytes).await.unwrap();
    }
    drop(input);
    let send = speakers(&mut server, &mut child, grant);
    let receive = async {
        for discontinuity in [false, true] {
            let mut bytes = [0; 1948];
            client.read_exact(&mut bytes).await.unwrap();
            let frame = reader.decode(&bytes).unwrap();
            assert_eq!(frame.discontinuity, discontinuity);
            assert!(frame
                .samples()
                .chunks_exact(2)
                .all(|pair| pair == [1234, -4321]));
        }
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(send, receive)
    })
    .await
    .unwrap();
    assert!(result.is_err()); // EOF retires this session.
    process::retire(&mut child).await;
}

#[test]
fn t719_native_speaker_packets_reject_bounded_invalid_flags_and_sizes() {
    for size in [0, 1, 1920, 1922, 4096] {
        assert!(protocol::captured(&vec![0; size]).is_err());
    }
    for flag in 0..=255 {
        let mut bytes = [0; 1921];
        bytes[0] = flag;
        assert_eq!(protocol::captured(&bytes).is_ok(), flag <= 1);
    }
}

#[tokio::test]
async fn t720_native_clock_request_and_packet_transfer_are_explicit_and_atomic() {
    use blent_config::audio::ClockSample;
    let token = "a".repeat(64);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    peer.write_all(&[b"BLAUREQ2".as_slice(), token.as_bytes(), &[7, 1, 1, 1]].concat())
        .await
        .unwrap();
    let (_, caps, _, _, clocked) = protocol::accept(&listener, &token, Direction::Microphone)
        .await
        .unwrap();
    assert!(clocked);
    let grant = AudioSession::new(Direction::Microphone)
        .start_with_clock(
            AudioProfile::new(Direction::Microphone),
            caps,
            true,
            0,
            clocked,
        )
        .unwrap();
    let clock = ClockSample {
        epoch: 3,
        frames: 96000,
        nanos: 2_000_000_001,
    };
    let packet = grant
        .encode_with_clock(0, 1, &[123; 480], Some(clock))
        .unwrap();
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    let mut bad = packet.clone();
    bad[28..36].fill(0);
    assert!(protocol::packet(&mut bad.as_slice(), &mut reader)
        .await
        .is_err());
    for size in [0, 27, 28, 51, 52, 1011] {
        assert!(protocol::packet(&mut &packet[..size], &mut reader)
            .await
            .is_err());
    }
    let native = protocol::packet(&mut packet.as_slice(), &mut reader)
        .await
        .unwrap();
    assert_eq!(native.len(), 985);
    assert_eq!(ClockSample::decode(&native[1..25]).unwrap(), Some(clock));
    assert_eq!(&native[25..], &packet[52..]);
    assert!(protocol::packet(&mut packet.as_slice(), &mut reader)
        .await
        .is_err());
    let mut captured = vec![1];
    captured.extend_from_slice(&ClockSample::encode(Some(clock)).unwrap());
    captured.extend_from_slice(&[1; 1920]);
    let block = protocol::captured(&captured).unwrap();
    assert_eq!(block.clock, Some(clock));
    assert!(block.discontinuity);
    captured[1..9].fill(0);
    assert!(protocol::captured(&captured).is_err());
}

#[tokio::test]
async fn t720_clocked_speaker_transport_preserves_native_counter_and_stereo() {
    use blent_config::audio::ClockSample;
    use tokio::io::AsyncReadExt;
    let (mut client, mut server) = sockets().await;
    let grant = AudioSession::new(Direction::Speakers)
        .start_with_clock(
            AudioProfile::new(Direction::Speakers),
            AudioCapabilities {
                speakers: true,
                speech: true,
                ..Default::default()
            },
            true,
            0,
            true,
        )
        .unwrap();
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    let clock = ClockSample {
        epoch: 1,
        frames: 480,
        nanos: 10_000_001,
    };
    let mut child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(
            &[
                &[0][..],
                &ClockSample::encode(Some(clock)).unwrap(),
                &[1; 1920],
            ]
            .concat(),
        )
        .await
        .unwrap();
    drop(input);
    let send = speakers_with_clock(&mut server, &mut child, grant, true);
    let receive = async {
        let mut bytes = [0; 1972];
        client.read_exact(&mut bytes).await.unwrap();
        let block = reader.decode(&bytes).unwrap();
        assert_eq!(block.clock, Some(clock));
        assert!(block.samples().iter().all(|s| *s == 257));
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(send, receive)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    process::retire(&mut child).await;
}
