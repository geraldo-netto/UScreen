//! T418: actual C producer, exec descriptor handoff, libavcodec retained buffers.
use super::shared;
use crate::encoder_io::StopSignal;
use crate::raw_memory::{Mapping, Reader};
use crate::raw_socket::Socket;
use blent_config::raw_frame::*;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Producer {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    socket: Socket,
    _directory: tempfile::TempDir,
}

impl Producer {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("producer");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = Command::new("cc")
            .args([
                "-O1",
                "-g",
                "-fsanitize=address,undefined",
                "-fno-pie",
                "-no-pie",
                "-I",
            ])
            .arg(root.join("evdi"))
            .arg(root.join("tests/raw_producer.c"))
            .arg(root.join("evdi/raw_ring.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (socket, endpoint) = Socket::pair().unwrap();
        let mut command = tokio::process::Command::new(binary);
        endpoint.attach(&mut command).unwrap();
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = command.as_std_mut().spawn().unwrap();
        Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            socket,
            _directory: directory,
        }
    }

    fn command(&mut self, command: &str) -> i32 {
        writeln!(self.input, "{command}").unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        line.trim().parse().unwrap()
    }

    fn reader(&mut self, dimensions: (u32, u32)) -> Reader {
        let reader = Reader::new(self.socket.clone(), dimensions, 4).unwrap();
        assert_eq!(self.command("service"), 1);
        reader
    }

    fn frame(&mut self, reader: &mut Reader, value: u8) -> ffmpeg_next::frame::Video {
        assert_eq!(self.command(&format!("frame {value}")), 1);
        shared::wrap(reader.next(&StopSignal::new().unwrap()).unwrap().unwrap()).unwrap()
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn t418_final_codec_reference_owns_slot_full_ring_and_resize() {
    let mut producer = Producer::new();
    let mut reader = producer.reader((64, 64));
    let mut frames = Vec::new();
    for value in 1..=4 {
        frames.push(producer.frame(&mut reader, value));
    }
    assert_eq!(
        producer.command("frame 99"),
        0,
        "T418: held ring must not be overwritten"
    );
    for (index, frame) in frames.iter().enumerate() {
        assert!(frame.data(0).iter().all(|value| *value == index as u8 + 1));
        unsafe {
            assert_eq!(
                ffmpeg_next::ffi::av_buffer_is_writable((*frame.as_ptr()).buf[0]),
                0
            );
        }
    }
    let mut retained = ffmpeg_next::frame::Video::empty();
    unsafe {
        assert_eq!(
            ffmpeg_next::ffi::av_frame_ref(retained.as_mut_ptr(), frames[0].as_ptr()),
            0
        );
    }
    frames.remove(0);
    assert_eq!(
        producer.command("frame 99"),
        0,
        "T418: clone retains original slot"
    );
    drop(retained);
    let replacement = producer.frame(&mut reader, 9);
    assert_eq!(replacement.data(0)[0], 9);
    assert_eq!(producer.command("resize 66 32"), 1);
    // Old mapping stays alive across producer unmap/close and new descriptors.
    assert!(frames[0].data(0).iter().all(|value| *value == 2));
    let mut successor = producer.reader((66, 32));
    let next = producer.frame(&mut successor, 17);
    assert_eq!(next.width(), 66);
    assert_eq!(next.stride(0), 66);
    frames.clear();
    drop(replacement); // Old generation release cannot free successor slots.
    assert!(next.data(0).iter().all(|value| *value == 17));
    for value in 1..=3 {
        frames.push(producer.frame(&mut successor, value));
    }
    assert_eq!(producer.command("frame 99"), 0);
}

fn descriptor() -> Message {
    Message {
        kind: DESCRIPTOR,
        generation: 1,
        nonce: 1,
        layout: Layout::new(64, 64, 4).unwrap(),
        sequence: 0,
        slot: 0,
        captured_us: 0,
    }
}

fn memfd(message: Message, seals: bool) -> OwnedFd {
    let fd = unsafe {
        libc::memfd_create(
            c"t418".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    assert!(fd >= 0);
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    assert_eq!(
        unsafe { libc::ftruncate(fd.as_raw_fd(), message.layout.total_bytes as i64) },
        0
    );
    if seals {
        assert_eq!(
            unsafe {
                libc::fcntl(
                    fd.as_raw_fd(),
                    libc::F_ADD_SEALS,
                    libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL,
                )
            },
            0
        );
    }
    fd
}

#[test]
fn t418_malformed_descriptors_stale_slots_and_peer_crash_are_bounded() {
    let (a, b) = Socket::pair().unwrap();
    let message = descriptor();
    assert!(Mapping::open(message, memfd(message, false), a.clone()).is_err());
    let fd = memfd(message, true);
    assert!(Mapping::open(
        Message {
            layout: Layout::new(128, 128, 4).unwrap(),
            ..message
        },
        fd.try_clone().unwrap(),
        a.clone()
    )
    .is_err());
    let mapping = Mapping::open(message, fd.try_clone().unwrap(), a.clone()).unwrap();
    let frame = Message {
        kind: FRAME,
        sequence: 1,
        ..message
    };
    assert!(mapping.lease(frame).is_err());
    for changed in [
        Message { nonce: 2, ..frame },
        Message {
            generation: 2,
            ..frame
        },
        Message {
            slot: u32::MAX,
            ..frame
        },
        message,
    ] {
        assert!(mapping.lease(changed).is_err());
    }
    assert!(unsafe { libc::ftruncate(fd.as_raw_fd(), 0) } < 0);
    assert!(
        unsafe { libc::ftruncate(fd.as_raw_fd(), (message.layout.total_bytes * 2) as i64) } < 0
    );
    for length in [0, 1, 95, 97, 128] {
        a.send_bytes(&vec![0; length], Some(fd.as_raw_fd()))
            .unwrap();
        assert!(b.receive().is_err());
    }
    a.send(message, Some(fd.as_raw_fd())).unwrap();
    let (received, fds) = b.receive().unwrap().unwrap();
    assert_eq!(received, message);
    assert_eq!(fds.len(), 1);
    assert_ne!(
        unsafe { libc::fcntl(fds[0].as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    assert!(b.receive().unwrap().is_none());
    drop(a);
    drop(mapping);
    assert!(b.receive().is_err());
    assert!(b.send(message, None).is_err());
}

#[test]
fn t418_cancel_partial_startup_and_crash_preserve_retained_pixels() {
    let (a, b) = Socket::pair().unwrap();
    let mut reader = Reader::new(a, (64, 64), 4).unwrap();
    let stop = StopSignal::new().unwrap();
    let cancel = stop.clone();
    let worker = std::thread::spawn(move || reader.next(&stop).unwrap().is_none());
    cancel.request();
    assert!(worker.join().unwrap());
    drop(b);
    let mut producer = Producer::new();
    let mut reader = producer.reader((64, 64));
    let frame = producer.frame(&mut reader, 55);
    producer.child.kill().unwrap();
    producer.child.wait().unwrap();
    assert!(reader.next(&StopSignal::new().unwrap()).is_err());
    assert!(frame.data(0).iter().all(|value| *value == 55));
    drop(frame); // Final release after peer exit must not signal/abort this process.
}

#[test]
fn t418_real_encoder_accepts_shared_readonly_nv12() {
    let mut producer = Producer::new();
    let mut reader = producer.reader((64, 64));
    let mut encoder = super::Encoder::new("libx264", 64, 64, 60, 500, 20).unwrap();
    for value in 1..=12 {
        encoder.frame = producer.frame(&mut reader, value);
        let output = encoder.encode_prepared(value == 1).unwrap();
        assert!(!output.is_empty());
        assert!(!output[0].0.is_empty());
    }
}

#[test]
fn t418_shared_input_uses_requested_capacity_and_stops_without_fifo() {
    let mut producer = Producer::new();
    let mut input = shared::Input::open(
        std::path::Path::new("/nonexistent-t418-fifo"),
        Some(producer.socket.clone()),
        (64, 64),
        2,
    )
    .unwrap();
    assert_eq!(producer.command("service"), 1);
    let stop = StopSignal::new().unwrap();
    let mut frame = ffmpeg_next::frame::Video::empty();
    assert_eq!(producer.command("frame 14"), 1);
    assert!(input.read(&mut frame, &stop).unwrap());
    assert_eq!(producer.command("frame 15"), 1);
    assert_eq!(
        producer.command("frame 16"),
        0,
        "T418: configured two-slot bound"
    );
    assert!(input.read(&mut frame, &stop).unwrap());
    assert!(frame.data(0).iter().all(|value| *value == 15));
    stop.request();
    assert!(!input.read(&mut frame, &stop).unwrap());
    assert!(Reader::new(producer.socket.clone(), (1, 64), 4).is_err());
    assert!(Reader::new(producer.socket.clone(), (64, 64), 9).is_err());
}
