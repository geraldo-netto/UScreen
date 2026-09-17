//! T402: compare full allocation/fill/publication/release boundaries, including
//! the stock pool's reuse advantage. No decoder or encoded-throughput claim.
use super::Encoder;
use crate::media_storage::{Budget, MediaBytes};
use ffmpeg_next::codec::packet::Mut;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

fn cpu_time() -> u64 {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut time) },
        0
    );
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

fn allocate(encoder: &mut Encoder, size: usize, owned: bool) -> ffmpeg_next::Packet {
    let mut packet = ffmpeg_next::Packet::empty();
    unsafe {
        (*packet.as_mut_ptr()).size = size as i32;
        let context = encoder.inner.as_mut_ptr();
        let callback = if owned {
            (*context).get_encode_buffer.unwrap()
        } else {
            ffmpeg_next::ffi::avcodec_default_get_encode_buffer
        };
        assert_eq!(callback(context, packet.as_mut_ptr(), 0), 0);
    }
    packet
}

fn publish(encoder: &Encoder, packet: ffmpeg_next::Packet, owned: bool) -> MediaBytes {
    if owned {
        encoder.packet_storage.payload(packet).unwrap()
    } else {
        let data = packet.data().unwrap();
        crate::allocation_probe::copied(data.len());
        MediaBytes::copy_from_slice(data)
    }
}

fn check(data: &MediaBytes, size: usize, value: u8) {
    assert_eq!(data.len(), size);
    assert_eq!(data[0], value);
    assert_eq!(data[size - 1], value);
}

fn feed(
    encoder: &mut Encoder,
    size: usize,
    count: usize,
    owned: bool,
    held: usize,
    budget: &Arc<Budget>,
    queue: &mut VecDeque<(MediaBytes, u8)>,
) {
    for sequence in 0..count {
        let value = sequence as u8;
        let mut packet = allocate(encoder, size, owned);
        packet.data_mut().unwrap().fill(value);
        let data = publish(encoder, packet, owned);
        assert!(data.charge(budget));
        queue.push_back((data, value));
        if queue.len() > held {
            let (data, value) = queue.pop_front().unwrap();
            check(&data, size, value);
        }
    }
}

fn trial(
    encoder: &mut Encoder,
    size: usize,
    count: usize,
    owned: bool,
    held: usize,
) -> serde_json::Value {
    let mut queue = VecDeque::with_capacity(held + 1);
    let budget = Budget::new(32 * 1024 * 1024);
    let cpu = cpu_time();
    let start = Instant::now();
    let (_, counts) = crate::allocation_probe::measure(|| {
        feed(encoder, size, count, owned, held, &budget, &mut queue);
    });
    let nanos = start.elapsed().as_nanos();
    let cpu = cpu_time() - cpu;
    for (data, value) in &queue {
        assert!(data.iter().all(|byte| byte == value));
    }
    drop(queue);
    let (live, peak) = budget.usage();
    assert_eq!(live, 0);
    serde_json::json!({"owned": owned, "size": size, "packets": count, "held": held,
        "ns": nanos, "cpu_ns": cpu, "counts": counts, "peak_charged_bytes": peak})
}

#[test]
#[ignore = "T402 optional paired allocator/publication boundary benchmark"]
fn t402_packet_boundary_replay() {
    let mut trials = Vec::new();
    for size in [1024, 65536, 524288, 4 * 1024 * 1024] {
        for held in [0, 4] {
            let mut encoder = Encoder::new("libx264", 64, 64, 60, 500, 20).unwrap();
            trial(&mut encoder, size, 32, false, held);
            trial(&mut encoder, size, 32, true, held);
            let count = (128 * 1024 * 1024 / size).clamp(128, 16384);
            for repeat in 0..10 {
                for owned in [repeat % 2 == 0, repeat % 2 != 0] {
                    let mut row = trial(&mut encoder, size, count, owned, held);
                    row["trial"] = repeat.into();
                    trials.push(row);
                }
            }
        }
    }
    println!("T402_BOUNDARY {}", serde_json::to_string(&trials).unwrap());
}
