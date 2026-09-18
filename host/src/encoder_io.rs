use crate::media::Codec;
use crate::media_storage::MediaBytes as Bytes;

#[path = "encoder_fifo.rs"]
mod fifo;
#[cfg(feature = "inproc-encoder")]
pub(crate) use fifo::FifoReader;
pub(crate) use fifo::StopSignal;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Waiting {
    Data,
    Writer,
}
pub(crate) trait FrameSource: std::io::Read {
    fn wait(&mut self, waiting: Waiting, stop: &StopSignal) -> std::io::Result<bool>;
}
enum ReadStep {
    Data(usize),
    Wait(Waiting),
}
fn read_step(source: &mut impl std::io::Read, bytes: &mut [u8]) -> std::io::Result<ReadStep> {
    use std::io::ErrorKind;
    match source.read(bytes) {
        Ok(0) => Ok(ReadStep::Wait(Waiting::Writer)),
        Ok(count) => Ok(ReadStep::Data(count)),
        Err(error) if error.kind() == ErrorKind::Interrupted => Ok(ReadStep::Data(0)),
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(ReadStep::Wait(Waiting::Data)),
        Err(error) => Err(error),
    }
}

/// A logical packed frame whose storage can occupy separate planes.
pub(crate) trait FrameTarget {
    fn len(&self) -> usize;
    /// Nonempty writable suffix within one contiguous region at packed offset.
    fn chunk(&mut self, offset: usize) -> &mut [u8];
}
impl FrameTarget for [u8] {
    fn len(&self) -> usize {
        <[u8]>::len(self)
    }
    fn chunk(&mut self, offset: usize) -> &mut [u8] {
        &mut self[offset..]
    }
}

pub(crate) fn read_frame(
    fifo: &mut impl FrameSource,
    buf: &mut [u8],
    stop: &StopSignal,
) -> std::io::Result<bool> {
    read_frame_into(fifo, buf, stop)
}

/// Fill one complete logical frame, possibly across disjoint writable planes.
/// EOF resets the whole packed offset, including already filled planes. T226
/// also replaces the inode on interrupted writes; cancellation never submits
/// partial input. The target borrow holds its storage until the read ends.
pub(crate) fn read_frame_into<T: FrameTarget + ?Sized>(
    fifo: &mut impl FrameSource,
    target: &mut T,
    stop: &StopSignal,
) -> std::io::Result<bool> {
    let mut filled = 0;
    while filled < target.len() {
        if stop.requested() {
            return Ok(false);
        }
        match read_step(fifo, target.chunk(filled))? {
            ReadStep::Data(count) => filled += count,
            ReadStep::Wait(waiting) => {
                if waiting == Waiting::Writer {
                    filled = 0;
                }
                if !fifo.wait(waiting, stop)? {
                    return Ok(false);
                }
            }
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
    annex_b_offsets(data).collect()
}

/// Allocation-free scanner; offsets include a prefix whose header is incomplete.
pub(crate) fn annex_b_offsets(data: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut offset = 0;
    std::iter::from_fn(move || {
        while offset + 3 <= data.len() {
            if let Some(length) = annex_b_prefix_len(&data[offset..]) {
                let start = offset;
                offset += length;
                return Some((start, offset));
            }
            offset += 1;
        }
        None
    })
}

fn parameter_set_slot(header: u8, codec: Codec) -> Option<usize> {
    let (kind, types): (u8, &[u8]) = match codec {
        Codec::H264 => (header & 0x1f, &[7, 8]),
        Codec::Vp9 | Codec::Av1 => return None,
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

    impl FrameSource for ReconnectingWriter {
        fn wait(&mut self, _: Waiting, stop: &StopSignal) -> io::Result<bool> {
            Ok(!stop.requested())
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
        let stop = StopSignal::new().unwrap();
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
