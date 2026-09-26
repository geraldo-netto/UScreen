//! Bounded IVF framing for stock FFmpeg. Unlike live WebM clusters, IVF can
//! publish a completed packet without waiting for the next input frame.
use crate::media::{Codec, EncoderGeneration, VideoPacket};
use crate::media_storage::MediaBytes;
use crate::video_queue::MAX_FRAME_BYTES;
use anyhow::{ensure, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt};

mod av1;

pub(crate) struct IvfPacketizer {
    codec: Codec,
    config: Option<MediaBytes>,
    av1: av1::Av1State,
    sequences: crate::latency::LatencyTracker,
    generation: EncoderGeneration,
    time_base: (u32, u32),
    timestamp_us: Option<i64>,
}
impl IvfPacketizer {
    pub(crate) fn new(codec: Codec, sequences: crate::latency::LatencyTracker) -> Self {
        Self {
            codec,
            config: None,
            av1: Default::default(),
            sequences,
            generation: EncoderGeneration::new(),
            time_base: (0, 0),
            timestamp_us: None,
        }
    }
    pub(crate) fn codec_config(&self) -> Option<MediaBytes> {
        self.config.clone()
    }
    pub(crate) fn timestamp_us(&self) -> Option<i64> {
        self.timestamp_us
    }
    pub(crate) async fn read_from(
        &mut self,
        input: &mut (impl AsyncRead + Unpin),
    ) -> Result<(usize, Vec<VideoPacket>)> {
        let mut read = 0;
        if self.config.is_none() {
            let mut header = [0; 32];
            input
                .read_exact(&mut header)
                .await
                .context("Truncated IVF header")?;
            self.config = Some(configuration(&header, self.codec)?);
            self.time_base = (
                u32::from_le_bytes(header[20..24].try_into().unwrap()),
                u32::from_le_bytes(header[16..20].try_into().unwrap()),
            );
            read = 32;
        }
        let mut header = [0; 12];
        if input.read(&mut header[..1]).await? == 0 {
            return Ok((read, Vec::new()));
        }
        input
            .read_exact(&mut header[1..])
            .await
            .context("Truncated IVF frame header")?;
        let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        self.timestamp_us = blent_config::idle::timestamp_us(
            i128::from(u64::from_le_bytes(header[4..12].try_into().unwrap())),
            self.time_base.0,
            self.time_base.1,
        );
        ensure!(
            (1..=MAX_FRAME_BYTES).contains(&size),
            "Invalid IVF packet size"
        );
        let mut data = vec![0; size];
        input
            .read_exact(&mut data)
            .await
            .context("Truncated IVF packet")?;
        let is_idr = match self.codec {
            Codec::Vp9 => vp9_keyframe(&data)?,
            Codec::Av1 => match self.av1.prepare(&mut data)? {
                Some(keyframe) => keyframe,
                None => return Ok((read + 12 + size, Vec::new())),
            },
            _ => anyhow::bail!("Unsupported IVF codec"),
        };
        let packet = VideoPacket {
            data: MediaBytes::from(data),
            is_idr,
            seq: self.sequences.next_sequence(),
            codec_config: self.config.clone(),
            generation: self.generation.active.clone(),
        };
        Ok((read + 12 + size, vec![packet]))
    }
}
fn configuration(header: &[u8; 32], codec: Codec) -> Result<MediaBytes> {
    ensure!(
        &header[..8] == b"DKIF\0\0\x20\0",
        "Unsupported IVF version/header"
    );
    let (fourcc, id): (&[u8], u8) = match codec {
        Codec::Vp9 => (b"VP90", 3),
        Codec::Av1 => (b"AV01", 4),
        _ => anyhow::bail!("Unsupported IVF codec"),
    };
    ensure!(&header[8..12] == fourcc, "IVF/control codec mismatch");
    let width = u16::from_le_bytes(header[12..14].try_into().unwrap());
    let height = u16::from_le_bytes(header[14..16].try_into().unwrap());
    ensure!(
        (2..=4096).contains(&width) && (2..=4096).contains(&height),
        "Unsupported IVF dimensions"
    );
    let mut config = b"BLN1".to_vec();
    config.push(id);
    config.extend_from_slice(&u32::from(width).to_be_bytes());
    config.extend_from_slice(&u32::from(height).to_be_bytes());
    Ok(MediaBytes::from(config))
}

/// VP9 uncompressed header (profile 0, the supported 8-bit 4:2:0 stream).
/// show_existing_frame reuses references and cannot be a random-access point.
fn vp9_keyframe(data: &[u8]) -> Result<bool> {
    let header = *data.first().context("Empty VP9 frame")?;
    ensure!(header >> 4 == 8, "Unsupported VP9 marker/profile");
    if header & 0x08 != 0 {
        return Ok(false);
    }
    if header & 0x04 != 0 {
        return Ok(false);
    }
    ensure!(
        data.get(1..4) == Some(&[0x49, 0x83, 0x42]),
        "Invalid VP9 keyframe sync code"
    );
    Ok(header & 0x02 != 0)
}

#[cfg(test)]
mod tests;
