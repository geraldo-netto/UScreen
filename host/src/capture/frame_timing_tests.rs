use super::*;

#[tokio::test]
async fn t561_frame_logs_correlate_packets_and_only_accepted_acks() {
    if std::env::var_os("BLENT_T561_FRAME_LOG_TEST").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "capture::cli_encoder::frame_timing_tests::t561_frame_logs_correlate_packets_and_only_accepted_acks", "--nocapture"])
            .env("BLENT_T561_FRAME_LOG_TEST", "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    crate::test_logging::enable();
    let tracker = crate::latency::LatencyTracker::new();
    let evidence = tracker.encoder_started("fixture", (64, 64, 30, 20000, 18));
    let pictures = vec![
        vec![
            0, 0, 1, 0x67, 0x11, 0, 0, 1, 0x68, 0x22, 0, 0, 1, 0x65, 0x80,
        ],
        vec![0, 0, 1, 0x41, 0x80],
    ];
    let stream = crate::framed_annex_b::tests::stream(Codec::H264, &pictures);
    let (tx, mut rx) = crate::video_queue::channel(8, Default::default());
    read_loop(
        stream.as_slice(),
        tx,
        Default::default(),
        tracker.clone(),
        Codec::H264,
        evidence.clone(),
    )
    .await
    .unwrap();
    let first = rx.recv().await.unwrap();
    let second = rx.recv().await.unwrap();
    tracker.on_rendered_from(first.seq, 0, Some("wrong-decoder"));
    tracker.on_rendered_from(second.seq, 0, None);
    tracker.on_rendered_from(second.seq, 0, None);
    let log = crate::test_logging::text();
    let ready: Vec<_> = log
        .lines()
        .filter(|line| line.contains("Packet ready"))
        .collect();
    assert_eq!(ready.len(), 2, "T561: missing per-packet timing: {log}");
    for (index, line) in ready.iter().enumerate() {
        assert!(line.contains(&format!("sequence={index}")), "{line}");
        assert!(
            line.contains(&format!("media_pts_us=Some({index})")),
            "{line}"
        );
        assert!(
            line.contains(&format!("encoder_epoch={}", evidence.epoch)),
            "{line}"
        );
        assert!(line.contains("output_elapsed_us="), "{line}");
        assert!(line.contains("packet_bytes="), "{line}");
    }
    let acks: Vec<_> = log
        .lines()
        .filter(|line| line.contains("Render ACK received"))
        .collect();
    assert_eq!(
        acks.len(),
        1,
        "T561: wrong/duplicate ACK must not produce timing evidence: {log}"
    );
    assert!(acks[0].contains(&format!("sequence={}", second.seq)));
    assert!(acks[0].contains(&format!("encoder_epoch={}", evidence.epoch)));
    assert!(acks[0].contains("packet_ready_to_ack_us="));
}
