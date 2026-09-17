//! T389: isolated, real nonblocking FIFO-to-AVFrame replay; no display/encoder.
mod encoder_io;
#[path = "encoder_frame.rs"]
mod frame_input;
use encoder_io::{FifoReader, StopSignal};
use ffmpeg_next::frame::Video;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::{Arc, Barrier};
use std::time::Instant;

struct Counted {
    file: std::fs::File,
    reads: Arc<std::sync::atomic::AtomicU64>,
}
impl Read for Counted {
    fn read(&mut self, data: &mut [u8]) -> std::io::Result<usize> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.file.read(data)
    }
}
impl AsRawFd for Counted {
    fn as_raw_fd(&self) -> i32 { self.file.as_raw_fd() }
}
fn cpu() -> u64 {
    let mut stamp: libc::timespec = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut stamp) }, 0);
    stamp.tv_sec as u64 * 1_000_000_000 + stamp.tv_nsec as u64
}
fn usage() -> libc::rusage {
    let mut usage = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut usage) }, 0);
    usage
}
fn fifo() -> (tempfile::TempDir, std::fs::File, std::fs::File, i32) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("frames");
    let native = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
    let reader = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&path).unwrap();
    let writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_SETPIPE_SZ, 1024 * 1024); }
    let capacity = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_GETPIPE_SZ) };
    (folder, reader, writer, capacity)
}
fn old_copy(frame: &mut Video, data: &[u8]) {
    let (width, height) = (frame.width() as usize, frame.height() as usize);
    let (y_stride, uv_stride) = (frame.stride(0), frame.stride(1));
    baseline_copy_plane(frame.data_mut(0), y_stride, &data[..width * height], width, height);
    baseline_copy_plane(frame.data_mut(1), uv_stride, &data[width * height..], width, height / 2);
}
include!("baseline_copy.rs");
fn verify(frame: &Video, value: u8) {
    for plane in 0..2 {
        let data = frame.data(plane);
        for row in 0..(frame.height() as usize >> plane) {
            let offset = row * frame.stride(plane);
            assert_eq!(data[offset], value);
            assert_eq!(data[offset + frame.width() as usize - 1], value);
        }
        std::hint::black_box(data);
    }
}
fn prepare(mode: &str, input: &mut frame_input::RawInput, frame: &mut Video,
           reader: &mut impl encoder_io::FrameSource, bytes: &mut [u8], stop: &StopSignal) {
    if mode == "direct" {
        assert!(input.read(frame, reader, stop).unwrap());
        return;
    }
    assert!(encoder_io::read_frame(reader, bytes, stop).unwrap());
    frame_input::writable(frame).unwrap();
    if mode == "rows" { old_copy(frame, bytes); }
    else { frame_input::copy_nv12(frame, bytes); }
}
fn produce(mut writer: std::fs::File, size: usize, count: usize,
           sender: std::sync::mpsc::SyncSender<Instant>) -> u64 {
    let mut bytes = vec![0u8; size];
    let mut started = 0;
    for sequence in 0..count + 32 {
        if sequence == 32 { started = cpu(); }
        sender.send(Instant::now()).unwrap();
        bytes.fill(sequence as u8);
        writer.write_all(&bytes).unwrap();
    }
    cpu() - started
}
fn trial(width: u32, height: u32, count: usize, mode: &str, gate: Arc<Barrier>) -> serde_json::Value {
    let (_folder, reader, writer, capacity) = fifo();
    let reads = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut reader = FifoReader::new(Counted { file: reader, reads: reads.clone() });
    let (send, receive) = std::sync::mpsc::sync_channel(3);
    let size = width as usize * height as usize * 3 / 2;
    let producer = std::thread::spawn(move || produce(writer, size, count, send));
    let mut frame = Video::new(ffmpeg_next::format::Pixel::NV12, width, height);
    let mut input = frame_input::RawInput::default();
    let mut bytes = if mode == "direct" { Vec::new() } else { vec![0; size] };
    let stop = StopSignal::new().unwrap();
    for sequence in 0..32 {
        receive.recv().unwrap();
        prepare(mode, &mut input, &mut frame, &mut reader, &mut bytes, &stop);
        verify(&frame, sequence as u8);
    }
    gate.wait();
    let initial_reads = reads.load(std::sync::atomic::Ordering::Relaxed);
    let initial_usage = usage();
    let initial_cpu = cpu();
    let start = Instant::now();
    let mut ages = Vec::with_capacity(count);
    for sequence in 32..count + 32 {
        let captured = receive.recv().unwrap();
        prepare(mode, &mut input, &mut frame, &mut reader, &mut bytes, &stop);
        ages.push(captured.elapsed().as_nanos() as u64);
        verify(&frame, sequence as u8);
    }
    let elapsed = start.elapsed().as_nanos() as u64;
    let consumer_cpu = cpu() - initial_cpu;
    let final_usage = usage();
    let producer_cpu = producer.join().unwrap();
    serde_json::json!({"ns": elapsed, "cpu_ns": consumer_cpu + producer_cpu,
        "consumer_cpu_ns": consumer_cpu, "producer_cpu_ns": producer_cpu,
        "reads": reads.load(std::sync::atomic::Ordering::Relaxed) - initial_reads,
        "minor_faults": final_usage.ru_minflt - initial_usage.ru_minflt,
        "voluntary_switches": final_usage.ru_nvcsw - initial_usage.ru_nvcsw,
        "involuntary_switches": final_usage.ru_nivcsw - initial_usage.ru_nivcsw,
        "pipe_capacity": capacity, "y_stride": frame.stride(0), "uv_stride": frame.stride(1),
        "age_ns": ages})
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 6);
    let width = args[1].parse::<u32>().unwrap();
    let height = args[2].parse::<u32>().unwrap();
    let frames = args[3].parse::<usize>().unwrap();
    let sessions = args[4].parse::<usize>().unwrap();
    let mode = &args[5];
    assert!(["rows", "contiguous", "direct"].contains(&mode.as_str()));
    ffmpeg_next::init().unwrap();
    let barrier = Arc::new(Barrier::new(sessions));
    let rows = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..sessions).map(|_| {
            let barrier = barrier.clone();
            scope.spawn(move || trial(width, height, frames, mode, barrier))
        }).collect();
        threads.into_iter().map(|t| t.join().unwrap()).collect::<Vec<_>>()
    });
    println!("{}", serde_json::json!({"streams": rows}));
}
