//! T407: instrument the actual read boundary; counterpart adapters are archived.
use super::*;
use std::sync::{Arc, Barrier};
use std::time::Instant;

struct Reader<'a> {
    data: &'a [u8],
    chunk: usize,
}
impl Reader<'_> {
    fn new(data: &[u8], chunk: usize) -> Reader<'_> {
        Reader { data, chunk }
    }
    async fn next(&mut self, parser: &mut AnnexBPacketizer) -> (usize, Vec<VideoPacket>) {
        parser
            .read_from(&mut (&mut self.data).take(self.chunk as u64))
            .await
            .unwrap()
    }
}

fn run(codec: Codec, data: &[u8], chunk: usize, repeats: usize) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut packets = 0;
        for _ in 0..repeats {
            let mut parser = AnnexBPacketizer::new(codec, Default::default());
            let mut reader = Reader::new(data, chunk);
            loop {
                let (count, output) = reader.next(&mut parser).await;
                packets += std::hint::black_box(output).len();
                if count == 0 {
                    break;
                }
            }
        }
        packets
    })
}

fn cpu_ns() -> u64 {
    let mut clock = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut clock) },
        0
    );
    clock.tv_sec as u64 * 1_000_000_000 + clock.tv_nsec as u64
}

fn session(
    codec: Codec,
    data: Arc<Vec<u8>>,
    chunk: usize,
    repeats: usize,
    barrier: Arc<Barrier>,
) -> serde_json::Value {
    barrier.wait();
    let start = Instant::now();
    let cpu = cpu_ns();
    let (packets, counts) = crate::allocation_probe::measure(|| run(codec, &data, chunk, repeats));
    let cpu_ns = cpu_ns() - cpu;
    serde_json::json!({"packets": packets, "elapsed_ns": start.elapsed().as_nanos(), "cpu_ns": cpu_ns, "counts": counts})
}

fn profile(codec: Codec, name: &str, frames: usize, payload: usize, chunk: usize, repeats: usize) {
    let data = Arc::new(super::profile::fixture(codec, frames, payload));
    assert_eq!(run(codec, &data, chunk, 1), frames);
    for sessions in [1, 2, 4] {
        let barrier = Arc::new(Barrier::new(sessions));
        let handles: Vec<_> = (0..sessions)
            .map(|_| {
                let (data, barrier) = (data.clone(), barrier.clone());
                std::thread::spawn(move || session(codec, data, chunk, repeats, barrier))
            })
            .collect();
        let samples: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        for sample in &samples {
            assert_eq!(
                sample["packets"].as_u64().unwrap() as usize,
                frames * repeats
            );
        }
        println!(
            "T407_READ_PROFILE {}",
            serde_json::json!({
                "codec": codec.muxer(), "workload": name, "sessions": sessions,
                "input_bytes_per_session": data.len() * repeats, "chunk_bytes": chunk, "samples": samples,
            })
        );
    }
}

#[test]
#[ignore = "isolated synthetic read-boundary benchmark; invoke explicitly"]
fn t407_read_boundary_profile() {
    for codec in [Codec::H264, Codec::Hevc] {
        profile(codec, "fragmented", 40, 512, 7, 30);
        profile(codec, "dense", 400, 512, 16384, 40);
        profile(codec, "large_nal", 2, 1024 * 1024, 4096, 20);
    }
}
