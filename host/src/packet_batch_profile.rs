//! T416: publication metadata is bounded independently of pipe read size.
use super::*;
use serde_json::json;
use std::{
    sync::{Arc, Barrier},
    time::Instant,
};
use tokio::io::BufReader;

struct Fixture {
    raw: Vec<u8>,
    framed: Vec<u8>,
    frames: usize,
}
impl Fixture {
    fn new(frames: usize, size: usize) -> Self {
        let mut picture = vec![0, 0, 0, 1, 1, 0x80];
        picture.resize(size, 0x55);
        let packets = vec![picture; frames];
        Self {
            raw: packets.concat(),
            framed: tests::stream(Codec::H264, &packets),
            frames,
        }
    }
}

async fn replay(fixture: &Fixture, framed: bool, chunk: usize) -> (usize, usize, usize) {
    let mut parser = FramedAnnexB::new(Codec::H264, Default::default());
    let mut legacy = AnnexBPacketizer::new(Codec::H264, Default::default());
    let source = if framed {
        &fixture.framed
    } else {
        &fixture.raw
    };
    let mut input = BufReader::with_capacity(chunk, source.as_slice());
    let (sender, _slow_consumer) = crate::video_queue::channel(8, Default::default());
    let (mut count, mut peak, mut vector_bytes) = (0, 0, 0);
    loop {
        let (n, packets) = if framed {
            parser.read_from(&mut input).await.unwrap()
        } else {
            legacy.read_from(&mut input).await.unwrap()
        };
        peak = peak.max(packets.len());
        vector_bytes = vector_bytes.max(packets.capacity() * std::mem::size_of::<VideoPacket>());
        for packet in packets {
            assert_eq!(packet.seq as usize, count);
            count += 1;
            sender
                .send(packet)
                .unwrap_or_else(|_| panic!("T416: queue lost its consumer"));
        }
        if n == 0 {
            return (count, peak, vector_bytes);
        }
    }
}

#[tokio::test]
async fn t416_dense_packets_publish_singly_before_slow_consumer_admission() {
    let fixture = Fixture::new(10_000, 6);
    for chunk in [1, 7, 512 * 1024] {
        let (count, peak, _) = replay(&fixture, true, chunk).await;
        assert_eq!(count, fixture.frames);
        assert_eq!(
            peak, 1,
            "T416: metadata accumulated outside queue admission"
        );
    }
}

fn cpu_ns() -> u64 {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut value) },
        0
    );
    value.tv_sec as u64 * 1_000_000_000 + value.tv_nsec as u64
}

fn sample(fixture: Arc<Fixture>, framed: bool, barrier: Arc<Barrier>) -> serde_json::Value {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    barrier.wait();
    let start = Instant::now();
    let cpu = cpu_ns();
    let ((count, peak, vector), allocations) =
        crate::allocation_probe::measure(|| runtime.block_on(replay(&fixture, framed, 512 * 1024)));
    assert_eq!(count, fixture.frames);
    json!({"packets":count,"peak_batch_packets":peak,"peak_batch_vector_bytes":vector,
        "wall_ns":start.elapsed().as_nanos(),"cpu_ns":cpu_ns()-cpu,"allocation_counts":allocations})
}

fn batch(fixture: Arc<Fixture>, framed: bool, sessions: usize) -> serde_json::Value {
    let barrier = Arc::new(Barrier::new(sessions));
    let tasks = (0..sessions)
        .map(|_| {
            let (fixture, barrier) = (fixture.clone(), barrier.clone());
            std::thread::spawn(move || sample(fixture, framed, barrier))
        })
        .collect::<Vec<_>>();
    let samples = tasks
        .into_iter()
        .map(|task| task.join().unwrap())
        .collect::<Vec<_>>();
    json!({"framed":framed,"sessions":sessions,"samples":samples})
}

#[test]
#[ignore = "Opt-in T416 synthetic metadata/allocation measurement"]
fn t416_packet_batch_profile() {
    for (name, frames, size) in [
        ("minimum", 87_381, 6),
        ("dense", 1024, 512),
        ("large", 4, 1024 * 1024),
    ] {
        let fixture = Arc::new(Fixture::new(frames, size));
        for repeat in 0..3 {
            for sessions in [1, 2, 4] {
                for framed in [false, true] {
                    println!(
                        "T416_BATCH {}",
                        json!({"workload":name,"repeat":repeat,
                        "raw_bytes":fixture.raw.len(),"framed_bytes":fixture.framed.len(),
                        "measurement":batch(fixture.clone(),framed,sessions)})
                    );
                }
            }
        }
    }
}
