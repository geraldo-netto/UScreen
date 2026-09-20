use super::*;
use uscreen_config::negotiation::{DecoderChoice, Profile, StreamProfile};

fn launch(lease: Lease, evidence: Arc<EncoderEvidence>, tracker: LatencyTracker) -> Handle {
    super::launch(lease, evidence, tracker, Default::default())
}

fn evidence(tracker: &LatencyTracker) -> Arc<EncoderEvidence> {
    tracker.encoder_started_with_decoder(
        "libx264",
        (640, 480, 30, 20000, 18),
        Some(DecoderChoice {
            name: "fixture".into(),
            stream: StreamProfile {
                codec: "h264".into(),
                format: Profile {
                    profile: "baseline".into(),
                    level: 31,
                    depth: 8,
                },
            },
            low_latency: false,
            operating_rate: None,
        }),
    )
}

fn lease(directory: &Path) -> Lease {
    let fifo = directory.join("capture.nv12");
    super::super::helper::ensure_fifo(&fifo).unwrap();
    Lease::new(&fifo).unwrap()
}

async fn ack(
    handle: &Handle,
    tracker: &LatencyTracker,
    evidence: &Arc<EncoderEvidence>,
    seq: u32,
    pts: i64,
) {
    handle.note(seq, Some(pts), seq % 2 == 0);
    tracker.on_encoded_for(seq, evidence);
    tokio::time::advance(Duration::from_millis(10)).await;
    tracker.on_rendered_from(
        seq,
        100,
        Some(&evidence.decoder.as_ref().unwrap().receipt()),
    );
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn t492_controller_trials_current_receipts_then_restores_cadence_on_stall() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = LatencyTracker::new();
    let evidence = evidence(&tracker);
    let activity = tracker.encoder_activity(evidence.clone());
    let handle = launch(lease(dir.path()), evidence.clone(), tracker.clone());
    for seq in 0..=16 {
        ack(&handle, &tracker, &evidence, seq, i64::from(seq) * 200_000).await;
        tokio::time::advance(Duration::from_millis(190)).await;
    }
    assert!(
        handle.lease.path.exists(),
        "T492: measured baseline must start a sparse trial"
    );
    for seq in 17..=32 {
        tokio::time::advance(Duration::from_millis(300)).await;
        ack(
            &handle,
            &tracker,
            &evidence,
            seq,
            3_200_000 + i64::from(seq - 16) * 500_000,
        )
        .await;
        tokio::time::advance(Duration::from_millis(190)).await;
    }
    assert!(handle.lease.path.exists());
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::task::yield_now().await;
    assert!(
        !handle.lease.path.exists(),
        "T492: no ACK progress must restore five updates/s"
    );
    drop(activity);
}

#[tokio::test(start_paused = true)]
async fn t492_retired_stream_and_cancellation_cannot_publish_into_successor() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = LatencyTracker::new();
    let old = evidence(&tracker);
    let activity = tracker.encoder_activity(old.clone());
    let handle = launch(lease(dir.path()), old, tracker.clone());
    handle.lease.publish(500).unwrap();
    let next = Arc::new(lease(dir.path()));
    next.publish(500).unwrap();
    let retained = handle.lease.clone();
    drop(handle);
    assert!(retained.publish(500).is_err());
    assert!(next
        .contents()
        .unwrap()
        .contains(&format!(" {} ", next.token)));
    drop(activity);
    let current = evidence(&tracker);
    let activity = tracker.encoder_activity(current.clone());
    let handle = launch(lease(dir.path()), current, tracker);
    tokio::task::yield_now().await;
    drop(activity);
    tokio::task::yield_now().await;
    assert!(handle.task.is_finished());
    next.clear();
    assert!(!next.path.exists());
}

#[tokio::test(start_paused = true)]
async fn t492_unknown_decoder_disabled_setting_and_invalid_fifo_never_start() {
    let tracker = LatencyTracker::new();
    let known = evidence(&tracker);
    let mut config = CaptureConfig::default();
    assert!(start(&config, known.clone(), tracker.clone(), Default::default()).is_none());
    config.adaptive_idle = true;
    let unknown = tracker.encoder_started("libx264", known.format);
    assert!(start(&config, unknown, tracker.clone(), Default::default()).is_none());
    config.instance = u32::MAX;
    assert!(start(&config, known, tracker, Default::default()).is_none());
    let dir = tempfile::tempdir().unwrap();
    assert!(Lease::new(&dir.path().join("absent")).is_err());
    assert!(Lease::new(dir.path()).is_err());
}

#[tokio::test(start_paused = true)]
async fn t492_late_join_missing_timestamps_and_bounded_history_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = LatencyTracker::new();
    let evidence = evidence(&tracker);
    let handle = launch(lease(dir.path()), evidence.clone(), tracker.clone());
    let origin = Instant::now();
    for seq in 0..300 {
        handle.note(seq, Some(i64::from(seq)), false);
    }
    assert_eq!(handle.timings.lock().unwrap().len(), MAX_TIMINGS);
    assert!(take(&handle.timings, 0).is_none());
    assert_eq!(take(&handle.timings, 298).unwrap().sequence, 298);
    assert_eq!(handle.timings.lock().unwrap().len(), 1);
    handle.note(300, None, true);
    tracker.on_encoded_for(300, &evidence);
    tracker.on_rendered_from(300, 0, Some(&evidence.decoder.as_ref().unwrap().receipt()));
    let mut policy = Policy::new(true);
    consume(&mut policy, &handle.timings, &evidence, &mut 0, origin);
    assert_eq!(policy.phase, Phase::Rejected);
    for seq in 301..600 {
        tracker.on_encoded_for(seq, &evidence);
        tracker.on_rendered_from(seq, 0, Some(&evidence.decoder.as_ref().unwrap().receipt()));
    }
    let mut policy = Policy::new(true);
    consume(&mut policy, &handle.timings, &evidence, &mut 0, origin);
    assert_eq!(
        policy.phase,
        Phase::Rejected,
        "T492: discarded ACK history cannot certify a late join"
    );
    assert_eq!(micros(origin, origin - Duration::from_secs(1)), 0);
}

#[test]
fn t492_lease_files_are_private_bounded_expiring_and_inode_scoped() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let first = lease(dir.path());
    first.publish(500).unwrap();
    assert_eq!(
        std::fs::metadata(&first.path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let fields: Vec<u64> = first
        .contents()
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(
        &fields[..3],
        &[first.identity.0, first.identity.1, first.token]
    );
    assert_eq!(fields[4], 500);
    assert!(first.publish(1).is_err());
    first.publish(200).unwrap();
    assert!(!first.path.exists());
    std::fs::write(&first.path, vec![b'x'; 1000]).unwrap();
    assert_eq!(first.contents().unwrap().len(), 160);
    first.clear();
    assert!(first.path.exists(), "do not delete another owner's request");
    std::fs::remove_file(&first.path).unwrap();
    symlink(dir.path().join("absent"), &first.path).unwrap();
    assert!(first.contents().is_err());
    first.publish(500).unwrap();
    assert!(!first.path.is_symlink());
    std::fs::remove_file(&first.path).unwrap();
    std::fs::create_dir(&first.path).unwrap();
    assert!(first.publish(500).is_err());
}

#[test]
fn t492_helper_and_encoder_share_explicit_idle_option() {
    let dir = tempfile::tempdir().unwrap();
    for enabled in [false, true] {
        let config = CaptureConfig {
            adaptive_idle: enabled,
            ..Default::default()
        };
        let helper = super::super::helper::HelperProcess::new();
        let command = helper
            .command(&config, &dir.path().join("capture.nv12"))
            .unwrap();
        let args: Vec<_> = command.as_std().get_args().collect();
        assert_eq!(
            args.contains(&std::ffi::OsStr::new("--idle-control-file")),
            enabled
        );
        let encoder = super::super::cli_encoder::CliEncoder { config: &config };
        let command = encoder.encoder_command(64, 64, false).unwrap();
        let args: Vec<_> = command.as_std().get_args().collect();
        let expression = args
            .windows(2)
            .find(|pair| pair[0] == "-force_key_frames")
            .unwrap()[1];
        assert_eq!(expression.to_string_lossy().contains("+0.9"), enabled);
    }
}

#[tokio::test(start_paused = true)]
async fn t492_control_publish_error_stops_the_controller() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = LatencyTracker::new();
    let evidence = evidence(&tracker);
    let handle = launch(lease(dir.path()), evidence.clone(), tracker.clone());
    std::fs::create_dir(&handle.lease.path).unwrap();
    for seq in 0..=16 {
        ack(&handle, &tracker, &evidence, seq, i64::from(seq) * 200_000).await;
        tokio::time::advance(Duration::from_millis(190)).await;
    }
    assert!(handle.task.is_finished());
}

#[tokio::test]
async fn t492_completed_packets_retain_their_own_timing_before_broadcast() {
    use crate::media::Codec;
    let dir = tempfile::tempdir().unwrap();
    let tracker = LatencyTracker::new();
    let evidence = evidence(&tracker);
    let handle = Handle {
        timings: Timings::default(),
        lease: Arc::new(lease(dir.path())),
        task: tokio::spawn(std::future::pending()),
    };
    let timings = handle.timings.clone();
    let pictures = vec![
        vec![
            0, 0, 1, 0x67, 0x11, 0, 0, 1, 0x68, 0x22, 0, 0, 1, 0x65, 0x80,
        ],
        vec![0, 0, 1, 0x41, 0x80],
    ];
    let stream = crate::framed_annex_b::tests::stream(Codec::H264, &pictures);
    let (tx, mut rx) = crate::video_queue::channel(8, Arc::new(AtomicBool::new(false)));
    super::super::cli_encoder::read_loop_with_idle(
        stream.as_slice(),
        tx,
        Default::default(),
        tracker,
        Codec::H264,
        evidence.clone(),
        Some(handle),
    )
    .await
    .unwrap();
    for pts in 0..2 {
        let packet = rx.recv().await.unwrap();
        let timing = take(&timings, packet.seq).unwrap();
        assert_eq!(timing.pts_us, Some(pts));
        assert_eq!(timing.keyframe, pts == 0);
    }
    assert!(
        !evidence.active(),
        "T492: EOF retires both stream and idle controller"
    );
}

#[tokio::test(start_paused = true)]
async fn t492_new_viewer_invalidates_sparse_admission_even_without_missing_frames() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = LatencyTracker::new();
    let evidence = evidence(&tracker);
    let (tx, _initial) = crate::video_queue::channel(8, Arc::new(AtomicBool::new(false)));
    let handle = super::launch(lease(dir.path()), evidence, tracker, tx.viewer_epoch());
    tokio::task::yield_now().await;
    handle.lease.publish(500).unwrap();
    let _new_viewer = tx.subscribe();
    tokio::time::advance(Duration::from_millis(250)).await;
    tokio::task::yield_now().await;
    assert!(!handle.lease.path.exists());
    assert!(
        handle.task.is_finished(),
        "T492: reconnect requires fresh encoder admission"
    );
}
