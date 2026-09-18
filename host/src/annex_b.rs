//! Incremental Annex B access-unit assembly, shared NAL scanning in encoder_io.
use crate::media::{Codec, EncoderGeneration, VideoPacket};
use crate::media_storage::MediaBytes as Bytes;
use crate::video_queue::{MAX_CONFIG_BYTES, MAX_FRAME_BYTES};
use anyhow::{ensure, Result};
use bytes::BufMut;
use tokio::io::AsyncReadExt;

const READ_BYTES: usize = 512 * 1024;
const INPUT_BYTES: usize = MAX_FRAME_BYTES + READ_BYTES + 3;

// Instrument explicit copies without changing the production hot path.
macro_rules! copied {
    ($bytes:expr) => {
        #[cfg(test)]
        crate::allocation_probe::copied($bytes);
    };
}

// NAL unit types. Only the CLI path parses the bitstream itself; with the
// in-process encoder libavcodec hands back one complete access unit per frame.
//
// HEVC types are from H.265 Table 7-1. IRAP covers every type a decoder may
// start from, not only IDR: a stream may open on a CRA.
const HEVC_NAL_VCL_MAX: u8 = 31;
const HEVC_NAL_IRAP_MIN: u8 = 16;
const HEVC_NAL_IRAP_MAX: u8 = 23;
const HEVC_NAL_VPS: u8 = 32;
const HEVC_NAL_SPS: u8 = 33;
const HEVC_NAL_PPS: u8 = 34;
const HEVC_NAL_AUD: u8 = 35;
const HEVC_NAL_PREFIX_SEI: u8 = 39;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NalKind {
    Sps,
    Pps,
    Prefix,
    Vcl,
    Other,
}

const NAL_TYPE_NON_IDR: u8 = 1;
const NAL_TYPE_IDR: u8 = 5;
const NAL_TYPE_SEI: u8 = 6;
const NAL_TYPE_AUD: u8 = 9;
const NAL_TYPE_SPS: u8 = 7;
const NAL_TYPE_PPS: u8 = 8;
pub(crate) struct AnnexBPacketizer {
    buffer: Vec<u8>,
    /// Consumed bytes stay in place until more read capacity is needed.
    consumed: usize,
    /// First unchecked prefix position; retain three bytes across chunk boundaries.
    scan_from: usize,
    /// Start of the retained, incomplete NAL (relative to buffer).
    nal_start: Option<usize>,
    pending_access_unit: Vec<u8>,
    pending_has_vcl: bool,
    pending_has_idr: bool,
    config: Bytes,
    parameter_sets: std::collections::BTreeMap<u8, Vec<u8>>,
    sequences: crate::latency::LatencyTracker,
    generation: EncoderGeneration,
    codec: Codec,
}

impl AnnexBPacketizer {
    pub(crate) fn new(codec: Codec, sequences: crate::latency::LatencyTracker) -> Self {
        assert!(!codec.framed(), "Framed codecs do not use Annex B");
        Self {
            buffer: Vec::new(),
            consumed: 0,
            scan_from: 0,
            nal_start: None,
            pending_access_unit: Vec::new(),
            pending_has_vcl: false,
            pending_has_idr: false,
            config: Bytes::new(),
            parameter_sets: std::collections::BTreeMap::new(),
            sequences,
            generation: EncoderGeneration::new(),
            codec,
        }
    }

    /// Read directly into owned spare capacity. No zero-filled scratch buffer
    /// or scratch-to-parser payload copy. Cancellation drops this generation.
    pub(crate) async fn read_from(
        &mut self,
        stdout: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> Result<(usize, Vec<VideoPacket>)> {
        if self.buffer.capacity() == 0 {
            self.buffer.reserve_exact(READ_BYTES);
        }
        // Use existing spare space; requiring a full READ_BYTES after every
        // tiny read needlessly doubles the allocation for fragmented streams.
        self.prepare_input(4096);
        let count = stdout
            .read_buf(&mut (&mut self.buffer).limit(READ_BYTES))
            .await?;
        let packets = if count == 0 {
            self.finish()?
        } else {
            self.process_complete_nals(false)?
        };
        Ok((count, packets))
    }

    #[cfg(test)]
    pub(crate) fn push(&mut self, data: &[u8]) -> Vec<VideoPacket> {
        if data.len() <= READ_BYTES {
            return self.push_part(data);
        }
        data.chunks(READ_BYTES)
            .flat_map(|part| self.push_part(part))
            .collect()
    }

    #[cfg(test)]
    fn push_part(&mut self, data: &[u8]) -> Vec<VideoPacket> {
        self.prepare_input(data.len());
        copied!(data.len());
        self.buffer.extend_from_slice(data);
        self.process_complete_nals(false).unwrap()
    }

    fn prepare_input(&mut self, bytes: usize) {
        let retained = self.buffer.len() - self.consumed;
        // Reclaim a large consumed span while its tail is still small. Waiting
        // for capacity exhaustion could copy almost the next complete picture.
        let reclaim = self.consumed >= READ_BYTES && self.consumed >= retained;
        if self.buffer.capacity() - self.buffer.len() >= bytes && !reclaim {
            return;
        }
        let fits_after_compaction = self.buffer.capacity() - retained >= bytes;
        let exceeds_cap = self.buffer.len() + bytes > INPUT_BYTES;
        if self.consumed > 0 && (reclaim || fits_after_compaction || exceeds_cap) {
            copied!(retained);
            self.buffer.copy_within(self.consumed.., 0);
            self.buffer.truncate(retained);
            self.scan_from -= self.consumed;
            self.nal_start = self.nal_start.map(|start| start - self.consumed);
            self.consumed = 0;
        }
        reserve_bounded(&mut self.buffer, bytes, INPUT_BYTES);
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<VideoPacket>> {
        let mut out = self.process_complete_nals(true)?;
        self.emit_pending_access_unit(&mut out);
        Ok(out)
    }

    fn emit_pending_access_unit(&mut self, out: &mut Vec<VideoPacket>) {
        if let Some(packet) = self.take_pending_access_unit() {
            out.push(packet);
        }
    }

    fn config_ready(&self) -> bool {
        let required: &[u8] = match self.codec {
            Codec::H264 => &[NAL_TYPE_SPS, NAL_TYPE_PPS],
            Codec::Vp9 => unreachable!("VP9 uses IVF framing"),
            Codec::Hevc => &[HEVC_NAL_VPS, HEVC_NAL_SPS, HEVC_NAL_PPS],
        };
        required
            .iter()
            .all(|kind| self.parameter_sets.contains_key(kind))
    }

    pub(crate) fn codec_config(&self) -> Option<Bytes> {
        if self.config_ready() {
            Some(self.config.clone())
        } else {
            None
        }
    }

    fn process_complete_nals(&mut self, flush: bool) -> Result<Vec<VideoPacket>> {
        // Temporarily move the Vec so borrowed NALs can update assembly state.
        // Return its capacity without shifting the unconsumed tail each chunk.
        let input = std::mem::take(&mut self.buffer);
        let mut out = Vec::new();
        self.scan_nals(&input, &mut out)?;
        self.consumed = match (flush, self.nal_start) {
            (true, Some(start)) => {
                self.process_nal(&input[start..], &mut out)?;
                self.nal_start = None;
                self.scan_from = input.len();
                input.len()
            }
            (_, Some(start)) => start,
            (_, None) => input.len().saturating_sub(3),
        };
        // Up to three bytes may be the next split start code, not payload.
        ensure!(
            input.len() - self.consumed <= MAX_FRAME_BYTES + 3,
            "Annex B unfinished NAL exceeds the frame limit"
        );
        self.buffer = input;
        if self.pending_has_vcl && self.trailing_nal_starts_picture() {
            self.emit_pending_access_unit(&mut out);
        }
        Ok(out)
    }

    fn scan_nals(&mut self, input: &[u8], out: &mut Vec<VideoPacket>) -> Result<()> {
        let offset = self.scan_from;
        #[cfg(test)]
        crate::allocation_probe::scanned(input.len() - offset);
        let mut checked_until = offset;
        for (start, header) in crate::encoder_io::annex_b_offsets(&input[offset..]) {
            let start = offset + start;
            // Preserve the streaming contract: a trailing three-byte prefix
            // waits for a header byte, while four bytes already identify it.
            if start >= input.len().saturating_sub(3) {
                break;
            }
            if let Some(previous) = self.nal_start {
                self.process_nal(&input[previous..start], out)?;
            }
            self.nal_start = Some(start);
            checked_until = offset + header;
        }
        // Never rescan inside a prefix already accepted at the very end.
        self.scan_from = checked_until.max(input.len().saturating_sub(3));
        Ok(())
    }

    fn trailing_nal_starts_picture(&self) -> bool {
        let trailing = &self.buffer[self.consumed..];
        let Some(offset) = nal_header_offset(trailing, 0) else {
            return false;
        };
        let Some(&header) = trailing.get(offset) else {
            return false;
        };
        let (kind, _) = self.classify_nal(header);
        let vcl = kind == NalKind::Vcl;
        let prefix = matches!(kind, NalKind::Sps | NalKind::Pps | NalKind::Prefix);
        prefix || (vcl && self.starts_new_picture(trailing, offset))
    }

    fn classify_nal(&self, header: u8) -> (NalKind, bool) {
        match self.codec {
            Codec::H264 => Self::classify_h264(header & 0x1f),
            Codec::Vp9 => unreachable!("VP9 uses IVF framing"),
            Codec::Hevc => Self::classify_hevc((header >> 1) & 0x3f),
        }
    }

    fn classify_h264(kind: u8) -> (NalKind, bool) {
        let class = match kind {
            NAL_TYPE_SPS => NalKind::Sps,
            NAL_TYPE_PPS => NalKind::Pps,
            NAL_TYPE_AUD | NAL_TYPE_SEI => NalKind::Prefix,
            NAL_TYPE_NON_IDR..=NAL_TYPE_IDR => NalKind::Vcl,
            _ => NalKind::Other,
        };
        (class, kind == NAL_TYPE_IDR)
    }

    fn classify_hevc(kind: u8) -> (NalKind, bool) {
        let class = match kind {
            HEVC_NAL_VPS | HEVC_NAL_SPS => NalKind::Sps,
            HEVC_NAL_PPS => NalKind::Pps,
            HEVC_NAL_AUD | HEVC_NAL_PREFIX_SEI => NalKind::Prefix,
            0..=HEVC_NAL_VCL_MAX => NalKind::Vcl,
            _ => NalKind::Other,
        };
        // Every IRAP picture, including CRA, is a valid decoder join point.
        (
            class,
            (HEVC_NAL_IRAP_MIN..=HEVC_NAL_IRAP_MAX).contains(&kind),
        )
    }

    fn process_nal(&mut self, nal: &[u8], out: &mut Vec<VideoPacket>) -> Result<()> {
        ensure!(
            nal.len() <= MAX_FRAME_BYTES,
            "Annex B NAL exceeds the frame limit"
        );
        let Some(header_offset) = nal_header_offset(nal, 0) else {
            return Ok(());
        };
        match self.classify_nal(nal[header_offset]) {
            (NalKind::Sps | NalKind::Pps, _) => {
                self.remember_parameter_set(nal, header_offset, out)
            }
            (NalKind::Prefix, _) => {
                if self.pending_has_vcl {
                    self.emit_pending_access_unit(out);
                }
                self.extend_pending(nal)
            }
            (NalKind::Vcl, is_key) => self.append_picture_slice(nal, header_offset, is_key, out),
            (NalKind::Other, _) => self.extend_pending(nal),
        }
    }

    fn remember_parameter_set(
        &mut self,
        nal: &[u8],
        header_offset: usize,
        out: &mut Vec<VideoPacket>,
    ) -> Result<()> {
        // Flush with the original configuration before replacing it.
        if self.pending_has_vcl {
            self.emit_pending_access_unit(out);
        }
        let nal_type = match self.codec {
            Codec::H264 => nal[header_offset] & 0x1f,
            Codec::Vp9 => unreachable!("VP9 uses IVF framing"),
            Codec::Hevc => (nal[header_offset] >> 1) & 0x3f,
        };
        let previous = self.parameter_sets.get(&nal_type);
        let payload = &nal[header_offset..];
        if previous.is_some_and(|set| &set[4..] == payload) {
            return Ok(());
        }
        let total = self.config.len() - previous.map_or(0, Vec::len) + 4 + payload.len();
        ensure!(
            total <= MAX_CONFIG_BYTES,
            "Annex B codec configuration exceeds the wire limit"
        );
        let mut set = Vec::with_capacity(4 + payload.len());
        set.extend_from_slice(&[0, 0, 0, 1]);
        set.extend_from_slice(payload);
        copied!(set.len());
        self.parameter_sets.insert(nal_type, set);
        let mut config = Vec::with_capacity(total);
        for set in self.parameter_sets.values() {
            config.extend_from_slice(set);
        }
        copied!(config.len());
        self.config = config.into();
        Ok(())
    }

    fn extend_pending(&mut self, data: &[u8]) -> Result<()> {
        ensure!(
            data.len() <= MAX_FRAME_BYTES - self.pending_access_unit.len(),
            "Annex B access unit exceeds the wire limit"
        );
        reserve_bounded(&mut self.pending_access_unit, data.len(), MAX_FRAME_BYTES);
        copied!(data.len());
        self.pending_access_unit.extend_from_slice(data);
        Ok(())
    }

    fn prepend_configuration(&mut self) -> Result<()> {
        if !self.config_ready() {
            return Ok(());
        }
        let length = self.config.len() + self.pending_access_unit.len();
        ensure!(
            length <= MAX_FRAME_BYTES,
            "Annex B keyframe configuration exceeds the wire limit"
        );
        let mut data = Vec::with_capacity(length);
        data.extend_from_slice(&self.config);
        data.extend_from_slice(&self.pending_access_unit);
        copied!(length);
        self.pending_access_unit = data;
        Ok(())
    }

    fn append_picture_slice(
        &mut self,
        nal: &[u8],
        header_offset: usize,
        is_key: bool,
        out: &mut Vec<VideoPacket>,
    ) -> Result<()> {
        if self.pending_has_vcl && self.starts_new_picture(nal, header_offset) {
            self.emit_pending_access_unit(out);
        }
        // Insert headers before the first key slice, avoiding a second copy
        // of the large picture on emission. Existing prefix metadata stays after
        // the headers, exactly as with the previous final coalescing copy.
        if is_key && !self.pending_has_idr {
            self.prepend_configuration()?;
        }
        self.pending_has_idr |= is_key;
        self.extend_pending(nal)?;
        self.pending_has_vcl = true;
        Ok(())
    }

    /// Is this slice the first of a new picture?
    ///
    /// H.264 answers with `first_mb_in_slice == 0`, which needs the
    /// exp-Golomb reader. HEVC puts `first_slice_segment_in_pic_flag` in the
    /// very first bit after its two-byte header, so it is just a bit test.
    fn starts_new_picture(&self, nal: &[u8], header_offset: usize) -> bool {
        match self.codec {
            Codec::H264 => Self::first_mb_in_slice(nal, header_offset) == Some(0),
            Codec::Vp9 => unreachable!("VP9 uses IVF framing"),
            Codec::Hevc => nal.get(header_offset + 2).is_some_and(|b| b & 0x80 != 0),
        }
    }

    fn take_pending_access_unit(&mut self) -> Option<VideoPacket> {
        if !self.pending_has_vcl || self.pending_access_unit.is_empty() {
            self.pending_access_unit.clear();
            self.pending_has_vcl = false;
            self.pending_has_idr = false;
            return None;
        }

        let was_idr = self.pending_has_idr;
        self.pending_has_vcl = false;
        self.pending_has_idr = false;

        let au_data = std::mem::take(&mut self.pending_access_unit);

        let data = Bytes::from(au_data);
        let seq = self.sequences.next_sequence();
        Some(VideoPacket {
            data,
            is_idr: was_idr,
            seq,
            codec_config: self.codec_config(),
            generation: self.generation.active.clone(),
        })
    }

    fn first_mb_in_slice(nal: &[u8], header_offset: usize) -> Option<u32> {
        let payload = nal.get(header_offset + 1..)?;
        ExpGolombReader::new(payload).read_ue()
    }
}

/// Explicit growth cap avoids Vec's doubling past the assembly limits.
fn reserve_bounded(data: &mut Vec<u8>, extra: usize, limit: usize) {
    let needed = data.len() + extra;
    assert!(needed <= limit);
    if data.capacity() < needed {
        // Modest aligned headroom avoids doubling a large first slice just
        // for a short continuation NAL. Still grow geometrically when needed.
        let alignment = if needed >= 65536 { 4096 } else { 64 };
        let capacity = needed
            .max(data.capacity().saturating_mul(2))
            .next_multiple_of(alignment)
            .min(limit);
        data.reserve_exact(capacity - data.len());
    }
}

struct ExpGolombReader<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u8,
}

impl<'a> ExpGolombReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte: 0,
            bit: 0,
        }
    }

    fn read_ue(mut self) -> Option<u32> {
        let mut leading_zero_bits = 0u32;
        while self.read_bit()? == 0 {
            leading_zero_bits += 1;
            if leading_zero_bits > 31 {
                return None;
            }
        }

        let mut value = 1u32.checked_shl(leading_zero_bits)?;
        for shift in (0..leading_zero_bits).rev() {
            value |= (self.read_bit()? as u32) << shift;
        }
        Some(value - 1)
    }

    fn read_bit(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.byte)?;
        let value = (byte >> (7 - self.bit)) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Some(value)
    }
}

fn nal_header_offset(data: &[u8], start: usize) -> Option<usize> {
    let header = start + crate::encoder_io::annex_b_prefix_len(data.get(start..)?)?;
    (header < data.len()).then_some(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t080_next_picture_header_releases_complete_previous_picture() {
        for (codec, first, continuation, next, header_len) in [
            (
                Codec::H264,
                nal(NAL_TYPE_IDR, &[0x80, 0x11]),
                nal(NAL_TYPE_IDR, &[0x40, 0x22]),
                nal(NAL_TYPE_NON_IDR, &[0x80, 0x33]),
                5,
            ),
            (
                Codec::Hevc,
                hevc_slice(19, true),
                hevc_slice(19, false),
                hevc_slice(1, true),
                6,
            ),
        ] {
            let mut p = AnnexBPacketizer::new(codec, Default::default());
            for byte in first.iter().chain(&continuation) {
                assert!(
                    p.push(&[*byte]).is_empty(),
                    "additional slices belong to same picture"
                );
            }
            for byte in &next[..header_len] {
                assert!(
                    p.push(&[*byte]).is_empty(),
                    "need enough header to identify next picture"
                );
            }
            let out = p.push(&next[header_len..header_len + 1]);
            assert_eq!(
                out.len(),
                1,
                "complete picture waits for unnecessary future frame"
            );
            assert_eq!(out[0].data.as_ref(), [first, continuation].concat());
            assert_eq!(out[0].seq, 0);
            assert!(p.push(&next[header_len + 1..]).is_empty());
            let last = p.finish().unwrap();
            assert_eq!(last.len(), 1);
            assert_eq!(last[0].data.as_ref(), next);
            assert_eq!(last[0].seq, 1);
        }
    }
    #[test]
    fn t079_parameter_sets_replace_without_growing_or_resending() {
        for (codec, mut sets) in [
            (
                Codec::H264,
                vec![
                    nal(NAL_TYPE_SPS, &[0x64, 0, 0x80]),
                    nal(NAL_TYPE_PPS, &[0x80]),
                ],
            ),
            (
                Codec::Hevc,
                vec![
                    hevc_nal(HEVC_NAL_VPS, &[0x80]),
                    hevc_nal(HEVC_NAL_SPS, &[0x80]),
                    hevc_nal(HEVC_NAL_PPS, &[0x80]),
                ],
            ),
        ] {
            let mut packetizer = AnnexBPacketizer::new(codec, Default::default());
            let mut packets = Vec::new();
            for set in &sets {
                packetizer.process_nal(set, &mut packets).unwrap();
            }
            let initial = packetizer.codec_config().unwrap();
            for _ in 0..1000 {
                for set in &sets {
                    packetizer.process_nal(set, &mut packets).unwrap();
                }
                assert_eq!(
                    packetizer.codec_config().as_ref(),
                    Some(&initial),
                    "unchanged sets must not resend config"
                );
            }
            *sets[0].last_mut().unwrap() = 0x81;
            packetizer.process_nal(&sets[0], &mut packets).unwrap();
            assert_eq!(packetizer.codec_config().unwrap().as_ref(), sets.concat());
            assert_eq!(packetizer.config.len(), initial.len());
        }
    }
    fn nal(nal_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 1, nal_type];
        data.extend_from_slice(payload);
        data
    }
    /// HEVC NAL: two-byte header, type in bits 1..6 of the first byte.
    fn hevc_nal(nal_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 1, nal_type << 1, 1];
        data.extend_from_slice(payload);
        data
    }
    /// A slice NAL whose first payload bit is `first_slice_segment_in_pic_flag`.
    fn hevc_slice(nal_type: u8, first_in_pic: bool) -> Vec<u8> {
        hevc_nal(nal_type, &[if first_in_pic { 0x80 } else { 0x00 }, 0x00])
    }
    #[test]
    fn hevc_parameter_sets_and_keyframes_are_recognised() {
        // A NAL is only parsed once the next start code proves it complete,
        // so anything pushed last stays buffered until finish().
        let mut incomplete = AnnexBPacketizer::new(Codec::Hevc, Default::default());
        incomplete.push(&hevc_nal(HEVC_NAL_VPS, &[1, 2]));
        incomplete.push(&hevc_nal(HEVC_NAL_SPS, &[3, 4]));
        incomplete.finish().unwrap();
        assert!(
            incomplete.codec_config().is_none(),
            "config is not complete without a PPS"
        );

        let mut p = AnnexBPacketizer::new(Codec::Hevc, Default::default());
        let mut data = Vec::new();
        // VPS, SPS and PPS all belong to the decoder configuration.
        data.extend_from_slice(&hevc_nal(HEVC_NAL_VPS, &[1, 2]));
        data.extend_from_slice(&hevc_nal(HEVC_NAL_SPS, &[3, 4]));
        data.extend_from_slice(&hevc_nal(HEVC_NAL_PPS, &[5, 6]));
        data.extend_from_slice(&hevc_slice(19, true)); // IDR_W_RADL
        let mut out = p.push(&data);
        out.extend(p.finish().unwrap());
        assert!(p.codec_config().is_some(), "VPS+SPS+PPS should complete it");
        assert_eq!(out.len(), 1);
        assert!(out[0].is_idr, "IDR_W_RADL must be marked as a keyframe");
    }
    #[test]
    fn hevc_cra_counts_as_a_join_point() {
        // A decoder may start at any IRAP picture, not only an IDR. Treating
        // CRA as an ordinary frame would leave a joining client waiting for a
        // keyframe the encoder never sends.
        let mut p = AnnexBPacketizer::new(Codec::Hevc, Default::default());
        p.push(&hevc_slice(21, true)); // CRA_NUT
        let out = p.finish().unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].is_idr, "CRA is a valid random access point");
    }
    #[test]
    fn hevc_splits_pictures_on_the_first_slice_flag() {
        let mut p = AnnexBPacketizer::new(Codec::Hevc, Default::default());
        let mut data = Vec::new();
        data.extend_from_slice(&hevc_slice(1, true)); // picture 1 starts
        data.extend_from_slice(&hevc_slice(1, false)); // ...continues
        data.extend_from_slice(&hevc_slice(1, true)); // picture 2 starts
        let mut out = p.push(&data);
        out.extend(p.finish().unwrap());
        assert_eq!(out.len(), 2, "two pictures, not three slices");
    }
    #[test]
    fn packetizer_handles_start_code_split_across_reads() {
        let mut packetizer = AnnexBPacketizer::new(Codec::H264, Default::default());
        assert!(packetizer.push(&[0, 0]).is_empty());
        assert!(packetizer.push(&[0, 1, NAL_TYPE_IDR, 0x80]).is_empty());

        let out = packetizer.finish().unwrap();
        assert_eq!(out.len(), 1);
        // No SPS/PPS seen, so IDR is emitted as-is
        assert_eq!(&out[0].data[..], &[0, 0, 0, 1, NAL_TYPE_IDR, 0x80]);
    }
    #[test]
    fn packetizer_splits_multiple_access_units_in_one_buffer() {
        let mut packetizer = AnnexBPacketizer::new(Codec::H264, Default::default());
        let mut data = nal(NAL_TYPE_AUD, &[0x10]);
        data.extend_from_slice(&nal(NAL_TYPE_IDR, &[0x80]));
        data.extend_from_slice(&nal(NAL_TYPE_AUD, &[0x10]));
        data.extend_from_slice(&nal(NAL_TYPE_NON_IDR, &[0x80]));

        let out = packetizer.push(&data);
        assert_eq!(out.len(), 1);
        // No SPS/PPS seen, so IDR AU emitted as-is
        assert_eq!(
            &out[0].data[..],
            &[
                0,
                0,
                0,
                1,
                NAL_TYPE_AUD,
                0x10,
                0,
                0,
                0,
                1,
                NAL_TYPE_IDR,
                0x80
            ]
        );

        let out = packetizer.finish().unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            &out[0].data[..],
            &[
                0,
                0,
                0,
                1,
                NAL_TYPE_AUD,
                0x10,
                0,
                0,
                0,
                1,
                NAL_TYPE_NON_IDR,
                0x80
            ]
        );
    }
    #[test]
    fn packetizer_prepends_sps_pps_to_idr() {
        let mut packetizer = AnnexBPacketizer::new(Codec::H264, Default::default());
        let mut data = nal(NAL_TYPE_SPS, &[0x64, 0x00]);
        data.extend_from_slice(&nal(NAL_TYPE_PPS, &[0xac]));
        data.extend_from_slice(&nal(NAL_TYPE_IDR, &[0x80]));

        assert!(packetizer.push(&data).is_empty());
        let config = packetizer.codec_config().expect("codec config");
        assert_eq!(
            &config[..],
            &[
                0,
                0,
                0,
                1,
                NAL_TYPE_SPS,
                0x64,
                0x00,
                0,
                0,
                0,
                1,
                NAL_TYPE_PPS,
                0xac
            ]
        );

        let out = packetizer.finish().unwrap();
        assert_eq!(out.len(), 1);
        // IDR frame should now have SPS+PPS prepended
        assert_eq!(
            &out[0].data[..],
            &[
                // SPS
                0,
                0,
                0,
                1,
                NAL_TYPE_SPS,
                0x64,
                0x00,
                // PPS
                0,
                0,
                0,
                1,
                NAL_TYPE_PPS,
                0xac,
                // IDR
                0,
                0,
                0,
                1,
                NAL_TYPE_IDR,
                0x80
            ]
        );
    }
    #[test]
    fn packetizer_does_not_emit_partial_nals() {
        let mut packetizer = AnnexBPacketizer::new(Codec::H264, Default::default());
        let first = nal(NAL_TYPE_IDR, &[0x80, 0x11, 0x22]);

        assert!(packetizer.push(&first[..4]).is_empty());
        assert!(packetizer.push(&first[4..]).is_empty());

        let mut second = nal(NAL_TYPE_NON_IDR, &[0x80]);
        let out = packetizer.push(&second[..3]);
        assert!(out.is_empty());

        second.drain(..3);
        let out = packetizer.push(&second);
        // T080: the next header proves the preceding NAL and picture complete.
        assert_eq!(out.len(), 1);
        assert_eq!(&out[0].data[..], &first[..]);

        let out = packetizer.finish().unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(&out[0].data[..], &nal(NAL_TYPE_NON_IDR, &[0x80]));
    }
    fn t285_sei(codec: Codec, suffix: bool, marker: u8) -> Vec<u8> {
        // user_data_unregistered: sixteen UUID bytes plus one data byte.
        let mut payload = vec![5, 17];
        payload.extend_from_slice(b"0123456789abcdef");
        payload.extend_from_slice(&[marker, 0x80]);
        match codec {
            Codec::H264 => nal(6, &payload),
            Codec::Vp9 => unreachable!("VP9 uses IVF framing"),
            Codec::Hevc => hevc_nal(if suffix { 40 } else { 39 }, &payload),
        }
    }
    fn t285_pictures(codec: Codec, with_aud: bool) -> (Vec<u8>, Vec<u8>) {
        let first = match codec {
            Codec::H264 => nal(NAL_TYPE_NON_IDR, &[0x80, 0x11]),
            Codec::Vp9 => unreachable!("VP9 uses IVF framing"),
            Codec::Hevc => hevc_nal(1, &[0x80, 0x11]),
        };
        let mut next = if with_aud {
            match codec {
                Codec::H264 => nal(NAL_TYPE_AUD, &[0x10]),
                Codec::Vp9 => unreachable!("VP9 uses IVF framing"),
                Codec::Hevc => hevc_nal(HEVC_NAL_AUD, &[0x10]),
            }
        } else {
            Vec::new()
        };
        next.extend(t285_sei(codec, false, b'A'));
        next.extend(t285_sei(codec, false, b'B'));
        next.extend_from_slice(&first);
        if codec == Codec::Hevc {
            next.extend(t285_sei(codec, true, b'C'));
        }
        (first, next)
    }
    #[test]
    fn t285_prefix_sei_belongs_to_next_picture_across_fragmentation() {
        // Boundary rules also used by FFmpeg's h264_find_frame_end and
        // hevc_find_frame_end; prefix SEI starts a new AU, suffix SEI does not.
        for codec in [Codec::H264, Codec::Hevc] {
            for with_aud in [false, true] {
                let (first, next) = t285_pictures(codec, with_aud);
                let stream = [first.as_slice(), next.as_slice()].concat();
                for chunk_size in 1..=stream.len() {
                    let mut packetizer = AnnexBPacketizer::new(codec, Default::default());
                    let mut packets = Vec::new();
                    for chunk in stream.chunks(chunk_size) {
                        packets.extend(packetizer.push(chunk));
                    }
                    packets.extend(packetizer.finish().unwrap());
                    let actual: Vec<_> =
                        packets.iter().map(|packet| packet.data.as_ref()).collect();
                    assert_eq!(
                        actual,
                        [first.as_slice(), next.as_slice()],
                        "T285 {codec:?}, AUD={with_aud}, chunks={chunk_size}"
                    );
                }
            }
        }
    }
    #[test]
    fn t285_prefix_sei_header_releases_previous_picture_promptly() {
        for codec in [Codec::H264, Codec::Hevc] {
            let (first, _) = t285_pictures(codec, false);
            let sei = t285_sei(codec, false, b'A');
            let mut packetizer = AnnexBPacketizer::new(codec, Default::default());
            assert!(packetizer.push(&first).is_empty());
            let packets = packetizer.push(&sei[..5]);
            assert_eq!(
                packets.len(),
                1,
                "T285 {codec:?} prefix header must finish prior AU"
            );
            assert_eq!(packets[0].data.as_ref(), first.as_slice());
            assert!(packetizer.push(&sei[5..]).is_empty());
            assert!(packetizer.push(&first).is_empty());
            assert_eq!(
                packetizer.finish().unwrap()[0].data.as_ref(),
                [sei, first].concat()
            );
        }
    }
}

#[cfg(test)]
#[path = "packetizer_profile.rs"]
mod profile;

#[cfg(test)]
#[path = "packetizer_decode_tests.rs"]
mod decode_tests;

#[cfg(test)]
#[path = "packetizer_read_profile.rs"]
mod read_profile;
