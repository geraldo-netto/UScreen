//! T406: counting writer replay; no kernel input device is opened.
mod linux {
    pub const EV_SYN: u16 = 0;
    pub const SYN_REPORT: u16 = 0;
}
mod event_writer;
use event_writer::EventBatch;
use std::io::{self, Write};
use std::sync::{Arc, Barrier};
#[derive(Default)]
struct Counted { writes: u64, bytes: u64, hash: u64 }
impl Write for Counted {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        self.bytes += bytes.len() as u64;
        for byte in bytes { self.hash = self.hash.wrapping_mul(31).wrapping_add(u64::from(*byte)); }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
#[cfg(feature = "baseline")]
fn emit(batch: &mut EventBatch, sink: &mut Counted, event: &(u16, u16, i32)) {
    if event.0 == 0 && event.1 == 0 { batch.syn(sink).unwrap(); }
    else { batch.emit(sink, event.0, event.1, event.2).unwrap(); }
}
#[cfg(not(feature = "baseline"))]
fn emit(batch: &mut EventBatch, sink: &mut Counted, event: &(u16, u16, i32)) {
    if event.0 == 0 && event.1 == 0 { batch.finish(sink).unwrap(); }
    else { batch.push(event.0, event.1, event.2).unwrap(); }
}
fn replay(events: Vec<Vec<(u16, u16, i32)>>, rounds: usize, gate: Arc<Barrier>) -> serde_json::Value {
    let mut sinks: Vec<_> = events.iter().map(|_| Counted::default()).collect();
    let mut batches: Vec<_> = events.iter().map(|_| EventBatch::default()).collect();
    gate.wait();
    for _ in 0..rounds {
        for ((events, sink), batch) in events.iter().zip(&mut sinks).zip(&mut batches) {
            for event in events { emit(batch, sink, event); }
        }
    }
    serde_json::json!(sinks.into_iter().map(|sink| (sink.writes, sink.bytes, sink.hash)).collect::<Vec<_>>())
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let sessions: usize = args[1].parse().unwrap();
    let rounds: usize = args[2].parse().unwrap();
    let events: Vec<Vec<(u16, u16, i32)>> = serde_json::from_str(include_str!("events.json")).unwrap();
    let gate = Arc::new(Barrier::new(sessions + 1));
    let workers: Vec<_> = (0..sessions).map(|_| {
        let (events, gate) = (events.clone(), gate.clone());
        std::thread::spawn(move || replay(events, rounds, gate))
    }).collect();
    gate.wait();
    let lanes: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
    println!("{}", serde_json::json!({"sessions": sessions, "rounds": rounds, "lanes": lanes}));
}
