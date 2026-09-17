//! T405: isolated paced and idle FIFO replay. Includes the revision's reader.
#![allow(dead_code)]
mod reader;
use reader::{read_frame, FifoReader, StopSignal};
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Barrier};
use std::time::{Duration, Instant};
extern "C" {
    fn bench_write(fd: libc::c_int, bytes: *const u8, size: usize);
    fn bench_counts(counts: *mut u64);
}
struct Counted(File, Arc<AtomicU64>);
impl Read for Counted {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.1.fetch_add(1, Ordering::Relaxed);
        self.0.read(bytes)
    }
}
impl AsRawFd for Counted {
    fn as_raw_fd(&self) -> RawFd { self.0.as_raw_fd() }
}
fn source(path: &std::path::Path) -> File {
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path).unwrap()
}
fn writer(path: &std::path::Path) -> File {
    OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(path).unwrap()
}
fn nanos(clock: libc::clockid_t) -> u64 {
    let mut value = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { libc::clock_gettime(clock, &mut value) }, 0);
    value.tv_sec as u64 * 1_000_000_000 + value.tv_nsec as u64
}
fn usage() -> libc::rusage {
    let mut value = std::mem::MaybeUninit::uninit();
    assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, value.as_mut_ptr()) }, 0);
    unsafe { value.assume_init() }
}
fn produce(file: File, size: usize, frames: usize, epoch: Instant, start: Arc<Barrier>) -> [u64; 2] {
    let mut buffer = vec![0x5a; size];
    start.wait();
    let schedule = Instant::now();
    for sequence in 0..frames {
        std::thread::sleep((schedule + Duration::from_nanos(sequence as u64 * 1_000_000_000 / 60)).saturating_duration_since(Instant::now()));
        buffer[..8].copy_from_slice(&(sequence as u64).to_le_bytes());
        buffer[8..16].copy_from_slice(&(epoch.elapsed().as_nanos() as u64).to_le_bytes());
        unsafe { bench_write(file.as_raw_fd(), buffer.as_ptr(), buffer.len()) };
    }
    let mut counts = [0; 2];
    unsafe { bench_counts(counts.as_mut_ptr()) };
    counts
}
fn consume(file: File, size: usize, frames: usize, epoch: Instant, start: Arc<Barrier>, stop: Arc<StopSignal>) -> serde_json::Value {
    let reads = Arc::new(AtomicU64::new(0));
    let mut source = FifoReader::new(Counted(file, reads.clone()));
    let mut buffer = vec![0; size];
    let mut ages = Vec::with_capacity(frames);
    start.wait();
    for sequence in 0..frames.max(1) {
        let complete = read_frame(&mut source, &mut buffer, &stop).unwrap();
        if frames == 0 {
            assert!(!complete);
            break;
        }
        assert!(complete);
        let received = epoch.elapsed().as_nanos() as u64;
        assert_eq!(u64::from_le_bytes(buffer[..8].try_into().unwrap()), sequence as u64);
        let timestamp = u64::from_le_bytes(buffer[8..16].try_into().unwrap());
        assert!(buffer[16..].iter().all(|byte| *byte == 0x5a));
        ages.push(received - timestamp);
    }
    serde_json::json!({"reads": reads.load(Ordering::Relaxed), "ages_ns": ages, "ended_ns": epoch.elapsed().as_nanos() as u64})
}
fn cancel_after_idle(active: bool, epoch: Instant, stops: &[Arc<StopSignal>]) -> u64 {
    if active { return 0; }
    std::thread::sleep(Duration::from_secs(1));
    let at = epoch.elapsed().as_nanos() as u64;
    for stop in stops { stop.request(); }
    at
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sessions: usize = args[1].parse().unwrap();
    let size: usize = args[2].parse().unwrap();
    let frames: usize = args[3].parse().unwrap();
    let mode = &args[4];
    assert!([1, 2, 4].contains(&sessions) && size >= 16);
    let directory = tempfile::tempdir().unwrap();
    let epoch = Instant::now();
    let active = mode == "active";
    let start = Arc::new(Barrier::new(sessions * (1 + usize::from(active)) + 1));
    let mut consumers = Vec::new();
    let mut producers = Vec::new();
    let mut stops = Vec::new();
    let mut held_writers = Vec::new();
    let mut capacities = Vec::new();
    for lane in 0..sessions {
        let path = directory.path().join(lane.to_string());
        let file = source(&path);
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETPIPE_SZ, 1 << 20) };
        capacities.push(unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPIPE_SZ) });
        let output = (mode != "no-writer").then(|| writer(&path));
        let stop = StopSignal::new().unwrap();
        stops.push(stop.clone());
        let gate = start.clone();
        consumers.push(std::thread::spawn(move || consume(file, size, if active { frames } else { 0 }, epoch, gate, stop)));
        if active {
            let gate = start.clone();
            producers.push(std::thread::spawn(move || produce(output.unwrap(), size, frames, epoch, gate)));
        } else { held_writers.push(output); }
    }
    let cpu = nanos(libc::CLOCK_PROCESS_CPUTIME_ID);
    let previous = usage();
    start.wait();
    let cancellation = cancel_after_idle(active, epoch, &stops);
    let readers: Vec<_> = consumers.into_iter().map(|thread| thread.join().unwrap()).collect();
    let writers: Vec<_> = producers.into_iter().map(|thread| thread.join().unwrap()).collect();
    let cpu_ns = nanos(libc::CLOCK_PROCESS_CPUTIME_ID) - cpu;
    let after = usage();
    println!("{}", serde_json::json!({"cpu_ns": cpu_ns, "voluntary_switches": after.ru_nvcsw - previous.ru_nvcsw,
        "involuntary_switches": after.ru_nivcsw - previous.ru_nivcsw, "capacities": capacities,
        "cancel_ns": cancellation, "readers": readers, "writers_write_poll": writers}));
}
