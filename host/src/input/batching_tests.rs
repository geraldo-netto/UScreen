//! T406: native bytes and intentional SYN boundaries without desktop input.
use super::linux::*;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, RawFd};

#[derive(Default)]
struct Recorder {
    bytes: Vec<u8>,
    writes: Vec<usize>,
    flushes: usize,
}
impl Write for Recorder {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes.push(bytes.len());
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}
impl AsRawFd for Recorder {
    fn as_raw_fd(&self) -> RawFd {
        -1
    }
}
fn event_bytes(kind: u16, code: u16, value: i32) -> Vec<u8> {
    let mut bytes = vec![0; std::mem::size_of::<libc::input_event>()];
    let kind_at = std::mem::offset_of!(libc::input_event, type_);
    let code_at = std::mem::offset_of!(libc::input_event, code);
    let value_at = std::mem::offset_of!(libc::input_event, value);
    bytes[kind_at..kind_at + 2].copy_from_slice(&kind.to_ne_bytes());
    bytes[code_at..code_at + 2].copy_from_slice(&code.to_ne_bytes());
    bytes[value_at..value_at + 4].copy_from_slice(&value.to_ne_bytes());
    bytes
}
fn events(bytes: &[u8]) -> Vec<(u16, u16, i32)> {
    let size = std::mem::size_of::<libc::input_event>();
    assert_eq!(bytes.len() % size, 0);
    bytes
        .chunks_exact(size)
        .map(|bytes| {
            let read16 = |offset| u16::from_ne_bytes(bytes[offset..offset + 2].try_into().unwrap());
            let at = std::mem::offset_of!(libc::input_event, value);
            (
                read16(std::mem::offset_of!(libc::input_event, type_)),
                read16(std::mem::offset_of!(libc::input_event, code)),
                i32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap()),
            )
        })
        .collect()
}
#[test]
fn t406_pen_down_preserves_proximity_button_tip_boundaries() {
    let mut pen = UInputDevice::from_writer(Recorder::default());
    pen.inject_pen(11, 22, 333, -44, 55, 0, false, Some(true))
        .unwrap();
    let frames = [
        vec![
            (EV_KEY, BTN_TOOL_PEN, 1),
            (EV_ABS, ABS_X, 11),
            (EV_ABS, ABS_Y, 22),
            (EV_ABS, ABS_TILT_X, -44),
            (EV_ABS, ABS_TILT_Y, 55),
            (EV_SYN, SYN_REPORT, 0),
        ],
        vec![(EV_KEY, BTN_STYLUS, 1), (EV_SYN, SYN_REPORT, 0)],
        vec![
            (EV_KEY, BTN_TOUCH, 1),
            (EV_ABS, ABS_PRESSURE, 333),
            (EV_SYN, SYN_REPORT, 0),
        ],
    ];
    let expected: Vec<u8> = frames
        .iter()
        .flatten()
        .flat_map(|&(kind, code, value)| event_bytes(kind, code, value))
        .collect();
    assert_eq!(
        pen.file.bytes, expected,
        "T406: changed native event bytes or padding"
    );
    assert_eq!(pen.file.flushes, 3);
    assert_eq!(
        pen.file.writes,
        [6, 2, 3].map(|count| count * std::mem::size_of::<libc::input_event>()),
        "T406: one immediate write per existing SYN frame"
    );
}
fn replay() -> InjectDevices<Recorder> {
    let mut devices = InjectDevices {
        pen: Some(UInputDevice::from_writer(Recorder::default())),
        touch: Some(UInputDevice::from_writer(Recorder::default())),
        pointer: Some(UInputDevice::from_writer(Recorder::default())),
        ..InjectDevices::empty()
    };
    for eraser in [false, true] {
        for (action, button) in [
            (3, None),
            (5, None),
            (0, Some(true)),
            (2, Some(false)),
            (1, None),
            (6, None),
            (4, None),
        ] {
            devices.apply_pen(
                AbsoluteContact {
                    x: 101,
                    y: 202,
                    pressure: 777,
                },
                (12.0, -24.0),
                eraser,
                action,
                button,
            );
        }
    }
    for slot in 0..10 {
        devices
            .inject_touch(100 + slot as i32, 200, 300, 0, slot)
            .unwrap();
    }
    devices.inject_touch(999, 888, 777, 2, 9).unwrap();
    devices.inject_touch(0, 0, 0, 1, 0).unwrap();
    devices.apply_pen(
        AbsoluteContact {
            x: 303,
            y: 404,
            pressure: 555,
        },
        (0.0, 0.0),
        false,
        0,
        Some(true),
    );
    devices.release_all();
    devices.release_all();
    devices
}
fn recorded(devices: &InjectDevices<Recorder>) -> Vec<Vec<(u16, u16, i32)>> {
    [&devices.pen, &devices.touch, &devices.pointer]
        .into_iter()
        .map(|device| events(&device.as_ref().unwrap().file.bytes))
        .collect()
}
#[test]
fn t406_replay_writes_once_per_existing_sync_frame() {
    let devices = replay();
    for device in [&devices.pen, &devices.touch, &devices.pointer] {
        let file = &device.as_ref().unwrap().file;
        let count = events(&file.bytes)
            .iter()
            .filter(|&&(kind, code, _)| kind == EV_SYN && code == SYN_REPORT)
            .count();
        assert_eq!(
            file.writes.len(),
            count,
            "T406: repeated writes within one SYN frame"
        );
        assert_eq!(file.flushes, count);
    }
    assert_eq!(devices.active_slots, 0);
    assert_eq!(devices.touch_contacts, [None; 10]);
    assert!(!devices.pen_proximity && !devices.pen_button);
}

#[test]
fn t406_recorded_replay_preserves_every_native_byte() {
    // Captured from f0f92e1 after generic extraction, before output buffering.
    let expected: Vec<Vec<(u16, u16, i32)>> =
        serde_json::from_str(include_str!("../../tests/fixtures/t406-input-events.json")).unwrap();
    let devices = replay();
    assert_eq!(recorded(&devices), expected);
    for (device, events) in [&devices.pen, &devices.touch, &devices.pointer]
        .into_iter()
        .zip(expected)
    {
        let bytes: Vec<u8> = events
            .into_iter()
            .flat_map(|(kind, code, value)| event_bytes(kind, code, value))
            .collect();
        assert_eq!(
            device.as_ref().unwrap().file.bytes,
            bytes,
            "T406: native timestamp/padding bytes changed"
        );
    }
}
#[test]
fn t406_pending_events_wait_for_the_current_syn_boundary() {
    let mut device = UInputDevice::from_writer(Recorder::default());
    device.emit(EV_ABS, ABS_X, 123).unwrap();
    device.emit(EV_ABS, ABS_Y, 456).unwrap();
    assert!(
        device.file.bytes.is_empty(),
        "T406: emitted an incomplete SYN frame"
    );
    device.syn().unwrap();
    assert_eq!(
        events(&device.file.bytes),
        [
            (EV_ABS, ABS_X, 123),
            (EV_ABS, ABS_Y, 456),
            (EV_SYN, SYN_REPORT, 0)
        ]
    );
    assert_eq!(device.file.writes.len(), 1);
}

#[derive(Default)]
struct ShortWriter {
    sink: Recorder,
    steps: std::collections::VecDeque<Result<usize, io::ErrorKind>>,
    flush_error: Option<io::ErrorKind>,
}
impl Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self
            .steps
            .pop_front()
            .unwrap_or(Ok(bytes.len()))
            .map_err(io::Error::from)?
            .min(bytes.len());
        self.sink.write(&bytes[..count])
    }
    fn flush(&mut self) -> io::Result<()> {
        self.sink.flush()?;
        self.flush_error
            .take()
            .map_or(Ok(()), |error| Err(error.into()))
    }
}
impl AsRawFd for ShortWriter {
    fn as_raw_fd(&self) -> RawFd {
        -1
    }
}
fn xy(device: &mut UInputDevice<impl Write + AsRawFd>) {
    device.emit(EV_ABS, ABS_X, 123).unwrap();
    device.emit(EV_ABS, ABS_Y, 456).unwrap();
}
#[test]
fn t406_short_and_interrupted_writes_preserve_every_byte() {
    let writer = ShortWriter {
        steps: [Err(io::ErrorKind::Interrupted), Ok(7), Ok(1), Ok(31)].into(),
        ..Default::default()
    };
    let mut device = UInputDevice::from_writer(writer);
    xy(&mut device);
    device.syn().unwrap();
    let expected: Vec<_> = [
        (EV_ABS, ABS_X, 123),
        (EV_ABS, ABS_Y, 456),
        (EV_SYN, SYN_REPORT, 0),
    ]
    .into_iter()
    .flat_map(|(kind, code, value)| event_bytes(kind, code, value))
    .collect();
    assert_eq!(device.file.sink.bytes, expected);
    assert_eq!(device.file.sink.flushes, 1);
}
#[test]
fn t406_failed_batch_never_replays_a_delivered_prefix() {
    let writer = ShortWriter {
        steps: [
            Ok(super::event_writer::EVENT_BYTES),
            Err(io::ErrorKind::BrokenPipe),
        ]
        .into(),
        ..Default::default()
    };
    let mut device = UInputDevice::from_writer(writer);
    xy(&mut device);
    assert!(device.syn().is_err());
    device.emit(EV_ABS, ABS_Y, 789).unwrap();
    device.syn().unwrap();
    assert_eq!(
        events(&device.file.sink.bytes),
        [
            (EV_ABS, ABS_X, 123),
            (EV_ABS, ABS_Y, 789),
            (EV_SYN, SYN_REPORT, 0)
        ]
    );
}
#[test]
fn t406_zero_write_and_flush_errors_retire_the_batch() {
    for zero_write in [true, false] {
        let writer = ShortWriter {
            steps: if zero_write {
                [Ok(0)].into()
            } else {
                Default::default()
            },
            flush_error: (!zero_write).then_some(io::ErrorKind::Other),
            ..Default::default()
        };
        let mut device = UInputDevice::from_writer(writer);
        xy(&mut device);
        assert!(device.syn().is_err());
        let previous = device.file.sink.bytes.len();
        device.emit(EV_KEY, BTN_TOUCH, 0).unwrap();
        device.syn().unwrap();
        assert_eq!(
            events(&device.file.sink.bytes[previous..]),
            [(EV_KEY, BTN_TOUCH, 0), (EV_SYN, SYN_REPORT, 0)]
        );
    }
}
#[test]
fn t406_event_bound_never_splits_or_leaks_an_unsynchronized_frame() {
    let mut device = UInputDevice::from_writer(Recorder::default());
    for _ in 1..super::event_writer::MAX_EVENTS {
        device.emit(EV_ABS, ABS_X, 1).unwrap();
    }
    device.syn().unwrap();
    assert_eq!(
        device.file.writes,
        [super::event_writer::MAX_EVENTS * super::event_writer::EVENT_BYTES]
    );
    let previous = device.file.bytes.len();
    for _ in 0..super::event_writer::MAX_EVENTS {
        device.emit(EV_ABS, ABS_Y, 2).unwrap();
    }
    assert!(device.syn().is_err());
    assert_eq!(device.file.bytes.len(), previous);
    device.emit(EV_KEY, BTN_TOUCH, 0).unwrap();
    device.syn().unwrap();
    assert_eq!(
        events(&device.file.bytes[previous..]),
        [(EV_KEY, BTN_TOUCH, 0), (EV_SYN, SYN_REPORT, 0)]
    );
}
#[test]
fn t406_dropping_device_does_not_publish_an_unfinished_frame() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut device = UInputDevice::from_writer(file.reopen().unwrap());
    xy(&mut device);
    drop(device);
    assert!(std::fs::read(file.path()).unwrap().is_empty());
}
#[test]
fn t406_native_bytes_match_linux_input_header_layout() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("layout.c");
    let binary = directory.path().join("layout");
    std::fs::write(
        &source,
        r#"
#include <linux/input.h>
#include <stdio.h>
#include <string.h>
int main(void) {
    struct input_event event;
    memset(&event, 0, sizeof(event));
    event.type = 3; event.code = 26; event.value = -44;
    return fwrite(&event, sizeof(event), 1, stdout) == 1 ? 0 : 1;
}
"#,
    )
    .unwrap();
    let result = std::process::Command::new("cc")
        .args(["-Wall", "-Wextra", "-Werror"])
        .arg(source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = std::process::Command::new(binary).output().unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, event_bytes(EV_ABS, ABS_TILT_X, -44));
    let mut device = UInputDevice::from_writer(Recorder::default());
    device.emit(EV_ABS, ABS_TILT_X, -44).unwrap();
    device.syn().unwrap();
    assert_eq!(&device.file.bytes[..result.stdout.len()], result.stdout);
}
