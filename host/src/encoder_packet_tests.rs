//! T402: characterize the safe copy baseline and measure a test-only owner.
use super::Encoder;
use crate::media_storage::MediaBytes;
use ffmpeg_next::codec::packet::Ref;
use std::time::Instant;

struct PacketOwner(ffmpeg_next::Packet);
impl AsRef<[u8]> for PacketOwner {
    fn as_ref(&self) -> &[u8] {
        self.0.data().unwrap()
    }
}

#[test]
fn t402_large_encoded_packets_avoid_the_drain_payload_copy() {
    let mut encoder = Encoder::new("libx264", 1024, 1024, 60, 60000, 12).unwrap();
    let pixels = noisy_pixels();
    let (packets, counts) =
        crate::allocation_probe::measure(|| encoder.encode(&pixels, true).unwrap());
    assert!(
        packets.iter().any(|(data, _)| data.len() >= 65536),
        "T402: fixture must produce a large packet"
    );
    let small_bytes: usize = packets
        .iter()
        .filter(|(data, _)| data.len() < 65536)
        .map(|(data, _)| data.len())
        .sum();
    assert!(
        counts.explicit_copy_bytes <= small_bytes as u64,
        "T402: known large packet storage was copied: {} bytes",
        counts.explicit_copy_bytes
    );
}

fn noisy_pixels() -> Vec<u8> {
    let mut pixels = vec![0; 1024 * 1024 * 3 / 2];
    let mut random = 0x12345678u32;
    for byte in &mut pixels {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        *byte = random as u8;
    }
    pixels
}

fn decode_picture<T: AsRef<[u8]>>(packets: &[T]) -> Vec<u8> {
    let codec = ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264).unwrap();
    let mut decoder = ffmpeg_next::codec::context::Context::new()
        .decoder()
        .open_as(codec)
        .unwrap()
        .video()
        .unwrap();
    for data in packets {
        decoder
            .send_packet(&ffmpeg_next::Packet::copy(data.as_ref()))
            .unwrap();
    }
    decoder.send_eof().unwrap();
    let mut frame = ffmpeg_next::frame::Video::empty();
    decoder.receive_frame(&mut frame).unwrap();
    assert_eq!(frame.format(), ffmpeg_next::format::Pixel::YUV420P);
    let mut pixels = Vec::new();
    for plane in 0..3 {
        let divisor = if plane == 0 { 1 } else { 2 };
        let width = frame.width() as usize / divisor;
        for row in 0..frame.height() as usize / divisor {
            let start = row * frame.stride(plane);
            pixels.extend_from_slice(&frame.data(plane)[start..start + width]);
        }
    }
    pixels
}

#[test]
fn t402_delayed_large_packet_decodes_like_a_detached_copy() {
    let mut encoder = Encoder::new("libx264", 1024, 1024, 60, 60000, 12).unwrap();
    let mut pixels = noisy_pixels();
    let packets = encoder.encode(&pixels, true).unwrap();
    assert!(packets.iter().any(|(data, _)| data.len() >= 65536));
    let copies: Vec<_> = packets.iter().map(|(data, _)| data.to_vec()).collect();
    let delayed: Vec<_> = packets.iter().map(|(data, _)| data.clone()).collect();
    for value in 0..4 {
        pixels[0] = value;
        encoder.encode(&pixels, false).unwrap();
    }
    drop(packets);
    drop(encoder);
    std::thread::spawn(move || {
        let reference = decode_picture(&copies);
        assert_eq!(reference.len(), 1024 * 1024 * 3 / 2);
        assert_eq!(decode_picture(&delayed), reference);
    })
    .join()
    .unwrap();
}

#[test]
fn t402_delayed_bytes_survive_later_encode_and_encoder_retirement() {
    let mut encoder = Encoder::new("libx264", 64, 64, 60, 500, 20).unwrap();
    let first = encoder.encode(&vec![64; 64 * 64 * 3 / 2], true).unwrap();
    assert!(!first.is_empty());
    let snapshots: Vec<_> = first.iter().map(|(data, _)| data.to_vec()).collect();
    for shade in 128..160 {
        encoder
            .encode(&vec![shade; 64 * 64 * 3 / 2], false)
            .unwrap();
    }
    drop(encoder);
    let delayed: Vec<_> = first.iter().map(|(data, _)| data.clone()).collect();
    drop(first);
    // A cancelled producer and an unrelated consumer cannot retire these bytes.
    std::thread::spawn(move || {
        let codec = ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264).unwrap();
        let mut decoder = ffmpeg_next::codec::context::Context::new()
            .decoder()
            .open_as(codec)
            .unwrap()
            .video()
            .unwrap();
        for (data, expected) in delayed.iter().zip(&snapshots) {
            assert_eq!(
                data.as_ref(),
                expected.as_slice(),
                "T402: retained payload changed"
            );
            decoder
                .send_packet(&ffmpeg_next::Packet::copy(data))
                .unwrap();
        }
        decoder.send_eof().unwrap();
        let mut frame = ffmpeg_next::frame::Video::empty();
        decoder.receive_frame(&mut frame).unwrap();
        assert_eq!((frame.width(), frame.height()), (64, 64));
        assert!(
            (i16::from(frame.data(0)[0]) - 64).abs() <= 2,
            "T402: decoded old frame differs"
        );
    })
    .join()
    .unwrap();
}

fn prepared_packets(size: usize, count: usize) -> Vec<ffmpeg_next::Packet> {
    (0..count)
        .map(|index| {
            let mut packet = ffmpeg_next::Packet::new(size);
            packet.data_mut().unwrap().fill(index as u8);
            packet
        })
        .collect()
}

fn copy_packets(packets: Vec<ffmpeg_next::Packet>) -> Vec<MediaBytes> {
    packets
        .into_iter()
        .map(|packet| {
            let data = packet.data().unwrap();
            crate::allocation_probe::copied(data.len());
            MediaBytes::copy_from_slice(data)
        })
        .collect()
}

fn own_packets(packets: Vec<ffmpeg_next::Packet>) -> Vec<MediaBytes> {
    packets
        .into_iter()
        .map(|packet| {
            // Only this fixture's freshly allocated packets are used here. A codec
            // may return an AVBufferRef view and retain other packet-owned buffers.
            let capacity = unsafe { (*(*packet.as_ptr()).buf).size };
            MediaBytes::from_owner(PacketOwner(packet), capacity)
        })
        .collect()
}

fn verify<T: AsRef<[u8]>>(output: &[T], size: usize) {
    for (index, data) in output.iter().enumerate() {
        let data = data.as_ref();
        assert_eq!(data.len(), size);
        assert!(data.iter().all(|&byte| byte == index as u8));
    }
}

fn copied_trial(packets: Vec<ffmpeg_next::Packet>, size: usize) -> serde_json::Value {
    let start = Instant::now();
    let (output, counts) = crate::allocation_probe::measure(|| copy_packets(packets));
    let nanos = start.elapsed().as_nanos();
    verify(&output, size);
    let release = Instant::now();
    drop(output);
    serde_json::json!({"variant": "copy", "ns": nanos, "counts": counts,
        "release_ns": release.elapsed().as_nanos()})
}

fn owned_trial(packets: Vec<ffmpeg_next::Packet>, size: usize) -> serde_json::Value {
    let start = Instant::now();
    let (output, counts) = crate::allocation_probe::measure(|| own_packets(packets));
    let nanos = start.elapsed().as_nanos();
    verify(&output, size);
    let release = Instant::now();
    drop(output);
    serde_json::json!({"variant": "owner_prototype", "ns": nanos, "counts": counts,
        "release_ns": release.elapsed().as_nanos()})
}

#[test]
#[ignore = "T402 optional microbenchmark; does not change production ownership"]
fn t402_packet_storage_replay() {
    let mut trials = Vec::new();
    for size in [1024, 65536, 524288, 4 * 1024 * 1024] {
        let count = (16 * 1024 * 1024 / size).min(128);
        for trial in 0..20 {
            for variant in [trial % 2, 1 - trial % 2] {
                let packets = prepared_packets(size, count);
                // Report only the visible public view; it is not a generic
                // allocation-size contract for buffers produced by a codec.
                let view_bytes: usize = packets
                    .iter()
                    .map(|packet| unsafe { (*(*packet.as_ptr()).buf).size })
                    .sum();
                let mut row = if variant == 0 {
                    copied_trial(packets, size)
                } else {
                    owned_trial(packets, size)
                };
                row["size"] = size.into();
                row["packets"] = count.into();
                row["trial"] = trial.into();
                row["public_buffer_view_bytes"] = view_bytes.into();
                trials.push(row);
            }
        }
    }
    println!("T402_REPLAY {}", serde_json::to_string(&trials).unwrap());
}
