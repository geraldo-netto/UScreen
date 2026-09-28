use super::*;

fn key() -> Key {
    let decoders = serde_json::from_str(include_str!(
        "../../../testdata/decoder-capabilities-v2.json"
    ))
    .unwrap();
    Key {
        epoch: 1,
        decoders,
        encoder: Some("libx264".into()),
        bitrate: 4000,
        quality: 18,
        workers: 1,
        ten_bit: false,
    }
}
fn programs() -> Programs {
    Programs {
        ffmpeg: "ffmpeg".into(),
        ffprobe: "ffprobe".into(),
    }
}
#[tokio::test]
async fn t530_native_cpu_packets_format_selection_and_stale_epochs() {
    let key = key();
    let programs = programs();
    assert!(programs
        .inventory()
        .await
        .unwrap()
        .contains(&"libx264".into()));
    let selection = programs.select(key.clone()).await.unwrap();
    let chosen = selection
        .best(&key)
        .expect("T530: CPU fallback must match tablet format");
    assert!(chosen.advertised && chosen.initialized);
    assert_eq!(chosen.packets, 65);
    let measured = chosen.measurement.as_ref().unwrap();
    assert_eq!(measured.workers_effective, Some(1));
    assert!(measured.fps.is_finite() && measured.fps > 0.0);
    assert!(measured.first_us > 0);
    assert_eq!(measured.stream.codec, "h264");
    for field in 0..5 {
        let mut stale = key.clone();
        match field {
            0 => stale.epoch += 1,
            1 => stale.quality += 1,
            2 => stale.workers += 1,
            3 => stale.decoders.fps += 1,
            _ => stale.decoders.software = Some("a".repeat(64)),
        }
        assert!(selection.best(&stale).is_none());
    }
}
#[tokio::test]
async fn t530_absent_commands_failed_inspection_and_incompatible_profiles() {
    let mut p = programs();
    p.ffmpeg = "blent-t530-missing-executable".into();
    assert!(p.inventory().await.is_err());
    p = programs();
    p.ffprobe = "blent-t530-missing-inspector".into();
    let k = key();
    let rejected = p.select(k.clone()).await.unwrap();
    assert!(rejected.best(&k).is_none());
    assert!(rejected.attempts[0].initialized);
    assert!(rejected.attempts[0].error.is_some());
    let mut incompatible = k.clone();
    for decoder in &mut incompatible.decoders.details {
        decoder.profiles.clear();
    }
    let rejected = programs().select(incompatible.clone()).await.unwrap();
    assert!(rejected.best(&incompatible).is_none());
    assert!(rejected.attempts[0].measurement.is_some());
    assert!(rejected.attempts[0]
        .error
        .as_ref()
        .unwrap()
        .contains("compatible"));
}
#[test]
fn t530_bounded_inventory_requests_and_measurement_rejection() {
    assert_eq!(
        inventory(b" V....D libx264 CPU\n V....D libx264 duplicate\n A..... hevc_amf fake\n")
            .unwrap(),
        ["libx264"]
    );
    assert!(inventory(&vec![b'x'; 1_048_577]).is_err());
    for byte in 0..=255 {
        let _ = inventory(&[byte; 128]);
    }
    let mut k = key();
    k.encoder = None;
    k.workers = 0;
    let all = requests(&k).unwrap();
    assert_eq!(all.len(), 9);
    assert_eq!(all[0].encoder, "libx264");
    assert_eq!(all[0].workers, 1);
    k.ten_bit = true;
    assert!(requests(&k)
        .unwrap()
        .iter()
        .any(|r| r.format == Format::Main10));
    for value in [0, 1, 9, 10, 90, 91, 128, 129, u32::MAX] {
        invalid_values(value);
    }
    k = key();
    k.encoder = Some("unknown".into());
    assert!(validate(&k).is_err());
    k = key();
    k.epoch = 0;
    assert!(validate(&k).is_err());
    k = key();
    k.decoders.protocol = 1;
    assert!(validate(&k).is_err());
    let profile = StreamProfile {
        codec: "h264".into(),
        format: blent_config::negotiation::Profile {
            profile: "baseline".into(),
            level: 31,
            depth: 8,
        },
    };
    assert!(summarize(&[], profile.clone()).is_err());
    assert!(summarize(&(0..65).rev().collect::<Vec<_>>(), profile.clone()).is_err());
    assert_eq!(
        summarize(&(0..65).map(|v| v * 1000).collect::<Vec<_>>(), profile)
            .unwrap()
            .p95_us,
        1000
    );
}
fn invalid_values(value: u32) {
    let mut k = key();
    k.workers = value;
    assert_eq!(validate(&k).is_ok(), value <= 128);
    k = key();
    k.decoders.fps = value;
    assert_eq!(validate(&k).is_ok(), (10..=90).contains(&value));
}
#[tokio::test]
async fn t530_command_output_is_bounded_and_nonzero_exit_rejected() {
    let exe = std::env::current_exe().unwrap();
    assert!(
        bounded_output(exe.as_os_str(), &["--definitely-invalid"], 1024)
            .await
            .is_err()
    );
    assert!(bounded_output(exe.as_os_str(), &["--list"], 10)
        .await
        .is_err());
}

#[tokio::test]
async fn t530_automatic_probes_preserve_cpu_fallback_and_explicit_choices() {
    let mut k = key();
    k.encoder = None;
    k.workers = 0;
    let selection = programs().select(k.clone()).await.unwrap();
    assert_eq!(selection.attempts.len(), 9);
    assert!(selection.best(&k).is_some());
    for attempt in &selection.attempts {
        if !attempt.advertised {
            assert!(!attempt.initialized && attempt.measurement.is_none());
        }
        eprintln!(
            "T530 {}: advertised={}, initialized={}, packets={}, error={:?}",
            attempt.encoder,
            attempt.advertised,
            attempt.initialized,
            attempt.packets,
            attempt.error
        );
    }
    k.encoder = Some("h264_amf".into());
    let explicit = programs().select(k.clone()).await.unwrap();
    assert_eq!(explicit.attempts.len(), 1);
    assert_eq!(explicit.attempts[0].encoder, "h264_amf");
}

#[tokio::test]
async fn t530_reused_format_inspection_supports_bounded_native_samples() {
    let packets = crate::annex_b::decode_tests::encode_output(crate::media::Codec::H264, true);
    let mut reader = tokio::io::BufReader::new(packets.as_slice());
    let mut parser =
        crate::framed_annex_b::FramedAnnexB::new(crate::media::Codec::H264, Default::default());
    let (_, frames) = parser.read_from(&mut reader).await.unwrap();
    let stream = crate::probe_format::inspect(crate::media::Codec::H264, 64, 48, &frames[0])
        .await
        .unwrap();
    assert_eq!(stream.codec, "h264");
}

#[test]
fn t530_shared_sample_keeps_ivf_header_and_packet_size() {
    use crate::media::{Codec, EncoderGeneration, VideoPacket};
    let generation = EncoderGeneration::new();
    let frame = VideoPacket {
        data: vec![1, 2, 3, 4].into(),
        is_idr: true,
        seq: 1,
        codec_config: None,
        generation: generation.active.clone(),
    };
    for (codec, magic) in [(Codec::Vp9, b"VP90"), (Codec::Av1, b"AV01")] {
        let sample = crate::probe_format::sample(codec, 64, 48, &frame);
        assert_eq!(&sample[8..12], magic);
        assert_eq!(&sample[12..16], &[64, 0, 48, 0]);
        assert_eq!(u32::from_le_bytes(sample[32..36].try_into().unwrap()), 4);
        assert_eq!(&sample[44..], &[1, 2, 3, 4]);
    }
}
