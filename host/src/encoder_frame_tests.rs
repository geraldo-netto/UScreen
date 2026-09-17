use super::*;
use crate::encoder_io::Waiting;
use std::collections::VecDeque;
use std::io::{self, Read};
use std::ops::Range;

struct Source {
    chunks: VecDeque<Option<Vec<u8>>>,
    planes: Vec<Range<usize>>,
    direct: bool,
    stop_at_wait: bool,
}
impl Read for Source {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        let start = dst.as_mut_ptr() as usize;
        self.direct &= self
            .planes
            .iter()
            .any(|p| p.start <= start && start + dst.len() <= p.end);
        let Some(bytes) = self.chunks.pop_front().expect("T389 source exhausted") else {
            return Ok(0);
        };
        let count = dst.len().min(bytes.len());
        dst[..count].copy_from_slice(&bytes[..count]);
        if count < bytes.len() {
            self.chunks.push_front(Some(bytes[count..].to_vec()));
        }
        Ok(count)
    }
}
impl FrameSource for Source {
    fn wait(&mut self, _: Waiting, stop: &StopSignal) -> io::Result<bool> {
        if self.stop_at_wait {
            stop.request();
        }
        Ok(!stop.requested())
    }
}
fn frame(width: u32, height: u32) -> Video {
    ffmpeg_next::init().unwrap();
    Video::new(ffmpeg_next::format::Pixel::NV12, width, height)
}
fn source(frame: &Video, chunks: Vec<Option<Vec<u8>>>) -> Source {
    Source {
        chunks: chunks.into(),
        planes: (0..2)
            .map(|i| {
                let plane = frame.data(i);
                let start = plane.as_ptr() as usize;
                start..start + plane.len()
            })
            .collect(),
        direct: true,
        stop_at_wait: false,
    }
}
fn assert_pixels(frame: &Video, value: u8) {
    let width = frame.width() as usize;
    for plane in 0..2 {
        for row in 0..(frame.height() as usize >> plane) {
            let start = row * frame.stride(plane);
            assert!(frame.data(plane)[start..start + width]
                .iter()
                .all(|b| *b == value));
        }
    }
}

#[test]
fn t389_aligned_reads_target_writable_planes_without_staging() {
    let mut frame = frame(64, 64);
    assert_eq!(frame.stride(0), 64);
    assert_eq!(frame.stride(1), 64);
    let mut source = source(&frame, vec![Some(vec![51; 64 * 64 * 3 / 2])]);
    let mut input = RawInput::default();
    assert!(input
        .read(&mut frame, &mut source, &StopSignal::new().unwrap())
        .unwrap());
    assert!(
        source.direct,
        "T389: FIFO read destination must be a writable AVFrame plane"
    );
    assert_eq!(
        input.staging.capacity(),
        0,
        "T389: aligned frames need no staging allocation"
    );
    assert_pixels(&frame, 51);
}

#[test]
fn t389_eof_between_planes_restarts_the_entire_frame() {
    let mut frame = frame(64, 64);
    let mut source = source(
        &frame,
        vec![
            Some(vec![11; 64 * 64 + 17]),
            None,
            Some(vec![77; 9]),
            Some(vec![77; 64 * 64 * 3 / 2 - 9]),
        ],
    );
    assert!(RawInput::default()
        .read(&mut frame, &mut source, &StopSignal::new().unwrap())
        .unwrap());
    assert_pixels(&frame, 77);
}

#[test]
fn t389_cancelled_partial_chroma_does_not_submit_a_frame() {
    let mut frame = frame(64, 64);
    let mut source = source(&frame, vec![Some(vec![11; 64 * 64 + 17]), None]);
    source.stop_at_wait = true;
    assert!(!RawInput::default()
        .read(&mut frame, &mut source, &StopSignal::new().unwrap())
        .unwrap());
}

#[test]
fn t389_direct_read_detaches_retained_encoder_planes() {
    let mut frame = frame(64, 64);
    copy_nv12(&mut frame, &vec![31; 64 * 64 * 3 / 2]);
    let mut retained = Video::empty();
    unsafe {
        assert_eq!(
            ffmpeg_next::ffi::av_frame_ref(retained.as_mut_ptr(), frame.as_ptr()),
            0
        );
    }
    let mut source = source(&frame, vec![Some(vec![99; 64 * 64 * 3 / 2])]);
    assert!(RawInput::default()
        .read(&mut frame, &mut source, &StopSignal::new().unwrap())
        .unwrap());
    assert_pixels(&retained, 31);
    assert_pixels(&frame, 99);
    assert_ne!(retained.data(0).as_ptr(), frame.data(0).as_ptr());
}

#[test]
fn t389_padded_planes_preserve_padding_and_packed_pixel_order() {
    let mut frame = frame(66, 34);
    assert!(frame.stride(0) > 66);
    for plane in 0..2 {
        frame.data_mut(plane).fill(193);
    }
    let mut source = source(&frame, vec![Some(vec![41; 66 * 34 * 3 / 2])]);
    let mut input = RawInput::default();
    assert!(input
        .read(&mut frame, &mut source, &StopSignal::new().unwrap())
        .unwrap());
    assert_pixels(&frame, 41);
    for plane in 0..2 {
        let stride = frame.stride(plane);
        for row in 0..(34 >> plane) {
            assert!(frame.data(plane)[row * stride + 66..(row + 1) * stride]
                .iter()
                .all(|b| *b == 193));
        }
    }
}

#[test]
fn t389_plane_copy_keeps_partial_rows_and_padding_untouched() {
    for stride in [4, 8] {
        let mut destination = [91; 24];
        copy_plane(&mut destination, stride, &[7; 10], 4, 3);
        assert_eq!(&destination[..4], &[7; 4]);
        assert_eq!(&destination[stride..stride + 4], &[7; 4]);
        assert!(destination[stride + 4..].iter().all(|b| *b == 91));
    }
}

#[test]
fn t389_direct_and_staged_inputs_encode_the_same_frames() {
    use crate::encoder::Encoder;
    for (width, height) in [(64, 64), (66, 34)] {
        let mut direct = Encoder::new("libx264", width, height, 60, 500, 20).unwrap();
        let mut staged = Encoder::new("libx264", width, height, 60, 500, 20).unwrap();
        let mut input = RawInput::default();
        let stop = StopSignal::new().unwrap();
        for (index, value) in [31, 123, 79, 197].into_iter().enumerate() {
            let data = vec![value; width as usize * height as usize * 3 / 2];
            let mut source = source(&direct.frame, vec![Some(data.clone())]);
            assert!(input.read(&mut direct.frame, &mut source, &stop).unwrap());
            let actual = direct.encode_prepared(index == 0).unwrap();
            let expected = staged.encode(&data, index == 0).unwrap();
            let payloads = |packets: Vec<(crate::media_storage::MediaBytes, bool)>| {
                packets
                    .into_iter()
                    .map(|(bytes, key)| (bytes.to_vec(), key))
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                payloads(actual),
                payloads(expected),
                "T389 packed/plane input changed encoded content"
            );
        }
    }
}
