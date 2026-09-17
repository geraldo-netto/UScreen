use crate::media::Codec;
use bytes::Bytes;

/// Fill `buf` completely, tolerating a FIFO that has no data yet and a writer
/// that has not opened it. Returns false if asked to stop before a whole frame
/// arrived — a partial frame must never reach the encoder, it would be encoded
/// as garbage.
pub(crate) fn read_frame(
    fifo: &mut impl std::io::Read,
    buf: &mut [u8],
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::io::Result<bool> {
    use std::sync::atomic::Ordering;
    let mut filled = 0;
    while filled < buf.len() {
        if stop.load(Ordering::Relaxed) {
            return Ok(false);
        }
        match fifo.read(&mut buf[filled..]) {
            Ok(0) => {
                // A reopened FIFO starts a new frame. Never prepend bytes
                // retained from a writer that closed partway through a frame.
                filled = 0;
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// Length of the Annex B prefix at the beginning of a slice.
pub(crate) fn annex_b_prefix_len(data: &[u8]) -> Option<usize> {
    if data.starts_with(&[0, 0, 0, 1]) {
        Some(4)
    } else if data.starts_with(&[0, 0, 1]) {
        Some(3)
    } else {
        None
    }
}

/// Start-code and NAL-header offsets, including an incomplete trailing prefix.
pub(crate) fn annex_b_starts(data: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut offset = 0;
    while offset + 3 <= data.len() {
        if let Some(length) = annex_b_prefix_len(&data[offset..]) {
            starts.push((offset, offset + length));
            offset += length;
        } else {
            offset += 1;
        }
    }
    starts
}

fn parameter_set_slot(header: u8, codec: Codec) -> Option<usize> {
    let (kind, types): (u8, &[u8]) = match codec {
        Codec::H264 => (header & 0x1f, &[7, 8]),
        Codec::Hevc => ((header >> 1) & 0x3f, &[32, 33, 34]),
    };
    types.iter().position(|&candidate| candidate == kind)
}

/// Collect the complete decoder configuration from an Annex B access unit.
/// Ignore AUD/SEI and slice NALs; never publish an incomplete parameter set.
pub(crate) fn extract_parameter_sets(au: &[u8], codec: Codec) -> Option<Bytes> {
    let starts = annex_b_starts(au);
    let mut sets: [Option<&[u8]>; 3] = [None; 3];
    for (n, &(start, header)) in starts.iter().enumerate() {
        let end = starts.get(n + 1).map_or(au.len(), |&(next, _)| next);
        if header + usize::from(codec == Codec::Hevc) >= end {
            continue;
        }
        let Some(slot) = parameter_set_slot(au[header], codec) else {
            continue;
        };
        sets[slot] = Some(&au[start..end]);
    }
    let count = if codec == Codec::Hevc { 3 } else { 2 };
    let mut config = Vec::new();
    for set in &sets[..count] {
        config.extend_from_slice((*set)?);
    }
    Some(Bytes::from(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        io::{self, Read},
        sync::{atomic::AtomicBool, Arc},
    };

    struct ReconnectingWriter(VecDeque<Option<Vec<u8>>>);
    impl Read for ReconnectingWriter {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self
                .0
                .pop_front()
                .expect("reader consumed more than one new frame")
            {
                None => Ok(0),
                Some(bytes) => {
                    let n = buf.len().min(bytes.len());
                    buf[..n].copy_from_slice(&bytes[..n]);
                    if n < bytes.len() {
                        self.0.push_front(Some(bytes[n..].to_vec()));
                    }
                    Ok(n)
                }
            }
        }
    }

    #[test]
    fn t027_discards_partial_frame_when_fifo_writer_disconnects() {
        let mut reader = ReconnectingWriter(VecDeque::from([
            Some(vec![1, 2]),
            None,
            Some(vec![3, 4, 5, 6]),
            Some(vec![7, 8, 9, 10]),
        ]));
        let stop = Arc::new(AtomicBool::new(false));
        let mut frame = [0; 4];
        assert!(read_frame(&mut reader, &mut frame, &stop).unwrap());
        assert_eq!(frame, [3, 4, 5, 6]);
        assert!(read_frame(&mut reader, &mut frame, &stop).unwrap());
        assert_eq!(frame, [7, 8, 9, 10]);
    }

    #[test]
    fn t028_extracts_hevc_vps_sps_pps() {
        let config = [
            0, 0, 0, 1, 64, 1, 42, 0, 0, 1, 66, 1, 43, 0, 0, 1, 68, 1, 44,
        ];
        let mut frame = config.to_vec();
        frame.extend_from_slice(&[0, 0, 1, 38, 1, 128]);
        assert_eq!(
            extract_parameter_sets(&frame, Codec::Hevc).as_deref(),
            Some(config.as_slice())
        );
    }

    #[test]
    fn t028_preserves_h264_parameter_sets() {
        let config = [0, 0, 1, 103, 42, 0, 0, 0, 1, 104, 43];
        let mut frame = config.to_vec();
        frame.extend_from_slice(&[0, 0, 1, 101, 128]);
        assert_eq!(
            extract_parameter_sets(&frame, Codec::H264).as_deref(),
            Some(config.as_slice())
        );
    }
}
