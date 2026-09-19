use super::*;
use std::{sync::atomic::Ordering, time::Duration};
use tokio::io::{AsyncWriteExt, BufReader};

#[test]
fn t497_extradata_bounds_and_malformed_numbers_are_rejected() {
    let limit = crate::video_queue::MAX_CONFIG_BYTES;
    for size in [0, 1, limit - 1, limit] {
        assert!(extradata(&format!(" {size}, 0xffffffff ")).is_ok());
    }
    for invalid in ["", "-1", "0x10", "18446744073709551616", "NaN"] {
        assert!(extradata(&format!("{invalid}, 0x1234")).is_err());
    }
    for size in limit + 1..limit + 1025 {
        assert!(extradata(&format!("{size}, 0x1234")).is_err());
    }
    for invalid in ["", "1234", "0x", "0x100000000", "0x-1", "0xzz", "0x1,extra"] {
        assert!(extradata(&format!("1,{invalid}")).is_err());
    }
    assert!(extradata("no comma").is_err());
}

pub(crate) fn stream(codec: Codec, packets: &[Vec<u8>]) -> Vec<u8> {
    let mut out = format!(
        "#tb 0: 1/1000000\n#media_type 0: video\n#codec_id 0: {}\n#dimensions 0: 64x64\n",
        codec.wire_name()
    )
    .into_bytes();
    for (index, packet) in packets.iter().enumerate() {
        out.extend(
            format!(
                "0, {index}, {index}, 0, {}, 0x{:08x}\n",
                packet.len(),
                adler(packet)
            )
            .bytes(),
        );
        out.extend_from_slice(packet);
    }
    out
}

fn pictures(codec: Codec) -> Vec<Vec<u8>> {
    match codec {
        Codec::H264 => vec![
            vec![
                0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22, 0, 0, 0, 1, 0x65, 0x80, 0x33,
            ],
            vec![0, 0, 0, 1, 0x41, 0x80, 0x44],
            vec![0, 0, 0, 1, 0x65, 0x80, 0x55],
        ],
        Codec::Hevc => vec![
            vec![
                0, 0, 0, 1, 0x40, 1, 0x11, 0, 0, 0, 1, 0x42, 1, 0x22, 0, 0, 0, 1, 0x44, 1, 0x33, 0,
                0, 0, 1, 0x26, 1, 0x80, 0x44,
            ],
            vec![0, 0, 0, 1, 2, 1, 0x80, 0x55],
            vec![0, 0, 0, 1, 0x26, 1, 0x80, 0x66],
        ],
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn t448_fragmented_packets_keep_configuration_sequence_and_generation() {
    for codec in [Codec::H264, Codec::Hevc] {
        let input = stream(codec, &pictures(codec));
        for chunk in [1, 3, 7, 512] {
            let mut input = BufReader::with_capacity(chunk, input.as_slice());
            let mut parser = FramedAnnexB::new(codec, Default::default());
            let mut frames = Vec::new();
            for seq in 0..3 {
                let (_, packets) = parser.read_from(&mut input).await.unwrap();
                assert_eq!(packets.len(), 1);
                assert_eq!(packets[0].seq, seq);
                assert_eq!(packets[0].is_idr, seq != 1);
                assert!(packets[0].codec_config.is_some());
                frames.extend(packets);
            }
            assert!(frames[2]
                .data
                .starts_with(frames[2].codec_config.as_ref().unwrap()));
            assert_eq!(parser.read_from(&mut input).await.unwrap().0, 0);
            assert!(frames[0].generation.load(Ordering::Acquire));
            drop(parser);
            assert!(!frames[0].generation.load(Ordering::Acquire));
        }
    }
}

async fn rejected(bytes: &[u8]) {
    let mut input = BufReader::with_capacity(3, bytes);
    let mut parser = FramedAnnexB::new(Codec::H264, Default::default());
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let (n, _) = parser.read_from(&mut input).await?;
            if n == 0 {
                return anyhow::Ok(());
            }
        }
    })
    .await
    .unwrap();
    assert!(result.is_err(), "T448 accepted malformed framing");
}

#[tokio::test]
async fn t448_rejects_corruption_truncation_and_malformed_metadata() {
    let packets = pictures(Codec::H264);
    let valid = stream(Codec::H264, &packets[..1]);
    for end in 0..valid.len() {
        // EOF immediately after complete stream headers is a valid empty stream.
        if end
            == valid
                .iter()
                .enumerate()
                .filter(|(_, b)| **b == b'\n')
                .nth(3)
                .unwrap()
                .0
                + 1
        {
            continue;
        }
        rejected(&valid[..end]).await;
    }
    let mut bad = valid.clone();
    *bad.last_mut().unwrap() ^= 1;
    rejected(&bad).await;
    for (from, to) in [
        ("h264", "hevc"),
        ("64x64", "65536x64"),
        ("1/1000000", "1/0"),
        ("video", "audio"),
        ("0, 0, 0, 0,", "1, 0, 0, 0,"),
        ("0, 0, 0, 0,", "0, x, 0, 0,"),
    ] {
        let original = String::from_utf8_lossy(&valid[..valid.len() - packets[0].len()]);
        let mut changed = original.replace(from, to).into_bytes();
        changed.extend_from_slice(&packets[0]);
        rejected(&changed).await;
    }
}

#[tokio::test]
async fn t448_bounds_lines_headers_packet_sizes_and_duplicate_timestamps() {
    rejected(&vec![b'#'; MAX_LINE + 1]).await;
    rejected("#software: fixture\n".repeat(MAX_HEADERS + 1).as_bytes()).await;
    let headers = stream(Codec::H264, &[]);
    for size in [0, crate::video_queue::MAX_FRAME_BYTES + 1, usize::MAX] {
        let mut input = headers.clone();
        input.extend(format!("0, 0, 0, 0, {size}, 0x00000000\n").bytes());
        rejected(&input).await;
    }
    let mut bytes = stream(Codec::H264, &pictures(Codec::H264)[..1]);
    let mut duplicate = stream(Codec::H264, &pictures(Codec::H264)[1..2]);
    bytes.extend(duplicate.drain(headers.len()..));
    rejected(&bytes).await;
    let multiple = pictures(Codec::H264).concat();
    rejected(&stream(Codec::H264, &[multiple])).await;
}

#[tokio::test]
async fn t448_large_packet_on_tiny_pipe_needs_no_successor_or_eof() {
    let mut packet = pictures(Codec::H264).remove(0);
    packet.resize(1024 * 1024, 0x55);
    let bytes = stream(Codec::H264, &[packet]);
    let (mut writer, reader) = tokio::io::duplex(64);
    let mut input = BufReader::new(reader);
    let mut parser = FramedAnnexB::new(Codec::H264, Default::default());
    tokio::time::timeout(Duration::from_secs(3), async {
        let (sent, received) = tokio::join!(writer.write_all(&bytes), parser.read_from(&mut input));
        sent.unwrap();
        let packets = received.unwrap().1;
        assert_eq!(packets.len(), 1);
        assert!(packets[0].data.len() >= 1024 * 1024);
    })
    .await
    .expect("T448: full pipe deadlocked");
}

#[tokio::test]
async fn t448_cancellation_retires_published_generation_mid_packet() {
    let bytes = stream(Codec::H264, &pictures(Codec::H264)[..1]);
    let mut parser = FramedAnnexB::new(Codec::H264, Default::default());
    let packet = parser
        .read_from(&mut bytes.as_slice())
        .await
        .unwrap()
        .1
        .remove(0);
    let (mut writer, reader) = tokio::io::duplex(64);
    writer
        .write_all(b"0, 1, 1, 0, 100, 0x00000000\n\0\0\0\x01")
        .await
        .unwrap();
    let task = tokio::spawn(async move { parser.read_from(&mut BufReader::new(reader)).await });
    tokio::task::yield_now().await;
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert!(!packet.generation.load(Ordering::Acquire));
    drop(writer);
}

#[test]
fn t448_checksum_uses_ffmpeg_zero_seed() {
    assert_eq!(
        adler(&[0, 0, 0, 1, 0x41, 0x9a, 0x20, 0x2e, 0x82, 0x30]),
        0x06cd01dc
    );
    assert_eq!(adler(b""), 0);
}

#[tokio::test]
async fn t448_metadata_only_packet_keeps_prefix_for_the_following_picture() {
    let prefix = vec![0, 0, 0, 1, 6, 0x55, 0x80];
    let bytes = stream(
        Codec::H264,
        &[prefix.clone(), vec![0, 0, 0, 1, 5, 0x80, 0x77]],
    );
    let mut input = bytes.as_slice();
    let mut parser = FramedAnnexB::new(Codec::H264, Default::default());
    assert!(parser.read_from(&mut input).await.unwrap().1.is_empty());
    let packet = parser.read_from(&mut input).await.unwrap().1.remove(0);
    assert_eq!(packet.seq, 0);
    assert!(
        packet.data.starts_with(&prefix),
        "T448: metadata-only packet lost its prefix"
    );
}

#[tokio::test]
async fn t448_stock_h264_hevc_framing_preserves_decoded_pictures_and_join_points() {
    use crate::annex_b::decode_tests::{decode, encode_output};
    for codec in [Codec::H264, Codec::Hevc] {
        let original = encode_output(codec, false);
        let expected = decode(codec, &original);
        let framed = encode_output(codec, true);
        let mut parser = FramedAnnexB::new(codec, Default::default());
        let mut input = BufReader::with_capacity(7, framed.as_slice());
        let mut frames = Vec::new();
        loop {
            let (n, packets) = parser.read_from(&mut input).await.unwrap();
            frames.extend(packets);
            if n == 0 {
                break;
            }
        }
        assert_eq!(frames.len(), 12);
        let data = frames
            .iter()
            .flat_map(|packet| packet.data.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(decode(codec, &data), expected);
        for index in [4, 8] {
            assert!(frames[index].is_idr);
            let suffix = frames[index..]
                .iter()
                .flat_map(|p| p.data.iter().copied())
                .collect::<Vec<_>>();
            assert_eq!(decode(codec, &suffix), expected[index * 64 * 48 * 3 / 2..]);
        }
    }
}
