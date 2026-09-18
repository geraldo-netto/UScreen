//! T384: deterministic, synthetic Annex B replay; no display/encoder/network access.
use super::*;

fn nal(codec: Codec, kind: u8, payload: usize, first: bool) -> Vec<u8> {
    let mut nal = vec![0, 0, 0, 1];
    match codec {
        Codec::H264 => nal.push(kind),
        Codec::Vp9 => unreachable!("Annex B fixture"),
        Codec::Hevc => nal.extend_from_slice(&[kind << 1, 1]),
    }
    nal.push(if first { 0x80 } else { 0x40 });
    nal.extend(std::iter::repeat_n(0x55, payload));
    nal
}

pub(super) fn fixture(codec: Codec, frames: usize, payload: usize) -> Vec<u8> {
    let (sets, idr, p): (&[u8], u8, u8) = match codec {
        Codec::H264 => (&[7, 8], 5, 1),
        Codec::Vp9 => unreachable!("Annex B fixture"),
        Codec::Hevc => (&[32, 33, 34], 19, 1),
    };
    let mut data = vec![0x55; 5]; // Ignore preamble bytes.
    for &kind in sets {
        data.extend(nal(codec, kind, 24, true));
    }
    for i in 0..frames {
        data.extend(nal(codec, if i % 10 == 0 { idr } else { p }, payload, true));
        data.extend(nal(codec, if i % 10 == 0 { idr } else { p }, 12, false));
    }
    data
}

fn collect(codec: Codec, data: &[u8], chunk: usize) -> Vec<VideoPacket> {
    let mut parser = AnnexBPacketizer::new(codec, Default::default());
    let mut packets = Vec::new();
    for part in data.chunks(chunk) {
        packets.extend(parser.push(part));
    }
    packets.extend(parser.finish().unwrap());
    packets
}

fn assert_same(expected: &[VideoPacket], actual: &[VideoPacket]) {
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.iter().zip(expected) {
        assert_eq!(a.data, b.data);
        assert_eq!((a.seq, a.is_idr), (b.seq, b.is_idr));
        assert_eq!(a.codec_config, b.codec_config);
    }
}

#[test]
fn t384_fragmentation_keeps_access_units_and_immutable_headers() {
    for codec in [Codec::H264, Codec::Hevc] {
        let data = fixture(codec, 24, 257);
        let expected = collect(codec, &data, data.len());
        assert_eq!(expected.len(), 24);
        for chunk in 1..=129 {
            assert_same(&expected, &collect(codec, &data, chunk));
        }
    }
}

fn replay(codec: Codec, data: &[u8], chunk: usize, repeats: usize) -> usize {
    let mut count = 0;
    for _ in 0..repeats {
        let mut parser = AnnexBPacketizer::new(codec, Default::default());
        for part in data.chunks(chunk) {
            count += std::hint::black_box(parser.push(part)).len();
        }
        count += std::hint::black_box(parser.finish().unwrap()).len();
    }
    count
}

fn profile(codec: Codec, name: &str, frames: usize, payload: usize, chunk: usize, repeats: usize) {
    let data = fixture(codec, frames, payload);
    assert_eq!(replay(codec, &data, chunk, 1), frames);
    for sample in 0..3 {
        let start = std::time::Instant::now();
        let (packets, counts) =
            crate::allocation_probe::measure(|| replay(codec, &data, chunk, repeats));
        let elapsed = start.elapsed().as_nanos();
        assert_eq!(packets, frames * repeats);
        println!(
            "T384_PROFILE {}",
            serde_json::json!({
                "codec": codec.muxer(), "workload": name, "sample": sample,
                "input_bytes": data.len() * repeats, "chunk_bytes": chunk, "packets": packets,
                "elapsed_ns": elapsed, "counts": counts
            })
        );
    }
}

#[test]
fn t384_packetizer_profile() {
    for codec in [Codec::H264, Codec::Hevc] {
        profile(codec, "fragmented", 40, 512, 7, 3);
        profile(codec, "dense", 400, 512, 16384, 4);
        profile(codec, "large_nal", 2, 1024 * 1024, 4096, 2);
    }
}

fn mixed_prefixes(codec: Codec) -> Vec<u8> {
    let data = fixture(codec, 12, 129);
    let starts = crate::encoder_io::annex_b_starts(&data);
    let mut mixed = vec![0x55; 5];
    for (index, &(start, _)) in starts.iter().enumerate() {
        let end = starts.get(index + 1).map_or(data.len(), |&(next, _)| next);
        mixed.extend_from_slice(&data[start + index % 2..end]);
    }
    mixed
}

#[test]
fn t384_three_and_four_byte_prefixes_survive_every_tail_boundary() {
    for codec in [Codec::H264, Codec::Hevc] {
        for tail in [vec![], vec![0], vec![0, 0], vec![0, 0, 1], vec![0, 0, 0, 1]] {
            let mut data = mixed_prefixes(codec);
            data.extend(tail);
            let expected = collect(codec, &data, data.len());
            assert_eq!(expected.len(), 12);
            for chunk in 1..=33 {
                assert_same(&expected, &collect(codec, &data, chunk));
            }
        }
    }
}

#[test]
fn t384_large_fragmented_nals_preserve_every_payload_byte() {
    for codec in [Codec::H264, Codec::Hevc] {
        let data = fixture(codec, 2, 1024 * 1024);
        let expected = collect(codec, &data, data.len());
        for chunk in [4093, 16384] {
            assert_same(&expected, &collect(codec, &data, chunk));
        }
    }
}
