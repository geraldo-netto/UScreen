//! T448: stock FFmpeg tee writes a framecrc line, then the unchanged encoded
//! packet, to the SAME pipe. Both slaves flush synchronously; no FIFO worker,
//! second pipe, raw muxer bitstream filter, or stdout read-boundary assumption.
use crate::{
    annex_b::AnnexBPacketizer,
    media::{Codec, VideoPacket},
    media_storage::MediaBytes,
};
use anyhow::{ensure, Context, Result};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

pub(crate) const TEE_OUTPUT: &str =
    "[f=framecrc:flush_packets=1]pipe:1|[f=data:flush_packets=1]pipe:1";
const MAX_LINE: usize = 512;
const MAX_HEADERS: usize = 32;

pub(crate) struct FramedAnnexB {
    codec: Codec,
    assembler: AnnexBPacketizer,
    headers: u8,
    header_count: usize,
    started: bool,
    previous_dts: Option<i64>,
    time_base: Option<(u32, u32)>,
    current_pts: Option<i64>,
    line: Vec<u8>,
    payload: Vec<u8>,
}

impl FramedAnnexB {
    pub(crate) fn new(codec: Codec, latency: crate::latency::LatencyTracker) -> Self {
        Self {
            codec,
            assembler: AnnexBPacketizer::new(codec, latency),
            headers: 0,
            header_count: 0,
            started: false,
            previous_dts: None,
            time_base: None,
            current_pts: None,
            line: Vec::with_capacity(MAX_LINE),
            payload: Vec::new(),
        }
    }

    pub(crate) fn codec_config(&self) -> Option<MediaBytes> {
        self.assembler.codec_config()
    }

    pub(crate) fn timestamp_us(&self) -> Option<i64> {
        let (num, den) = self.time_base?;
        uscreen_config::idle::timestamp_us(i128::from(self.current_pts?), num, den)
    }

    pub(crate) async fn read_from(
        &mut self,
        input: &mut (impl AsyncBufRead + Unpin),
    ) -> Result<(usize, Vec<VideoPacket>)> {
        let mut read = 0;
        loop {
            read_line(input, &mut self.line).await?;
            read += self.line.len();
            if self.line.is_empty() {
                ensure!(self.headers == 15, "Missing encoded stream header");
                return Ok((read, Vec::new()));
            }
            if self.line[0] != b'#' {
                break;
            }
            self.header()?;
        }
        ensure!(self.headers == 15, "Incomplete encoded stream header");
        self.started = true;
        let (size, checksum, dts, pts) = packet_header(&self.line)?;
        ensure!(
            self.previous_dts.is_none_or(|previous| dts > previous),
            "Nonmonotonic encoded packet"
        );
        self.previous_dts = Some(dts);
        self.current_pts = (dts == pts).then_some(pts);
        if self.payload.capacity() < size {
            self.payload.reserve_exact(size - self.payload.len());
        }
        self.payload.resize(size, 0);
        input
            .read_exact(&mut self.payload)
            .await
            .context("Truncated encoded packet")?;
        ensure!(
            adler(&self.payload) == checksum,
            "Encoded packet checksum mismatch"
        );
        let packets = self.assembler.complete_packet(&self.payload)?;
        Ok((read + size, packets))
    }

    fn header(&mut self) -> Result<()> {
        ensure!(!self.started, "Unexpected header after encoded packet");
        self.header_count += 1;
        ensure!(
            self.header_count <= MAX_HEADERS,
            "Too many encoded stream headers"
        );
        let text = std::str::from_utf8(&self.line)?.trim_end();
        let (key, value) = text
            .split_once(':')
            .context("Malformed encoded stream header")?;
        let value = value.trim();
        let bit = match key {
            "#codec_id 0" => {
                ensure!(value == self.codec.wire_name(), "Encoded codec mismatch");
                1
            }
            "#media_type 0" => {
                ensure!(value == "video", "Expected encoded video");
                2
            }
            "#dimensions 0" => {
                dimensions(value)?;
                4
            }
            "#tb 0" => {
                self.time_base = Some(time_base(value)?);
                8
            }
            "#software" | "#sar 0" => 0,
            "#extradata 0" => {
                extradata(value)?;
                0
            }
            _ => anyhow::bail!("Unexpected encoded stream header"),
        };
        ensure!(self.headers & bit == 0, "Duplicate encoded stream header");
        self.headers |= bit;
        Ok(())
    }
}

async fn read_line(input: &mut (impl AsyncBufRead + Unpin), line: &mut Vec<u8>) -> Result<()> {
    line.clear();
    loop {
        let available = input.fill_buf().await?;
        if available.is_empty() {
            ensure!(line.is_empty(), "Truncated encoded packet header");
            return Ok(());
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let length = newline.map_or(available.len(), |index| index + 1);
        ensure!(
            length <= MAX_LINE - line.len(),
            "Encoded packet header exceeds limit"
        );
        line.extend_from_slice(&available[..length]);
        input.consume(length);
        if newline.is_some() {
            return Ok(());
        }
    }
}

fn dimensions(value: &str) -> Result<()> {
    let (width, height) = value
        .split_once('x')
        .context("Missing encoded dimensions")?;
    ensure!(
        (2..=4096).contains(&width.parse::<u32>()?),
        "Invalid encoded width"
    );
    ensure!(
        (2..=4096).contains(&height.parse::<u32>()?),
        "Invalid encoded height"
    );
    Ok(())
}

fn time_base(value: &str) -> Result<(u32, u32)> {
    let (num, den) = value.split_once('/').context("Missing encoded time base")?;
    let (num, den) = (num.parse::<u32>()?, den.parse::<u32>()?);
    ensure!(num > 0 && den > 0, "Invalid encoded time base");
    Ok((num, den))
}

fn hex(value: &str) -> Result<u32> {
    Ok(u32::from_str_radix(
        value
            .trim()
            .strip_prefix("0x")
            .context("Missing checksum prefix")?,
        16,
    )?)
}

fn extradata(value: &str) -> Result<()> {
    let (size, checksum) = value.split_once(',').context("Missing extradata size")?;
    ensure!(
        size.trim().parse::<usize>()? <= crate::video_queue::MAX_CONFIG_BYTES,
        "Extradata exceeds limit"
    );
    hex(checksum)?;
    Ok(())
}

fn packet_header(line: &[u8]) -> Result<(usize, u32, i64, i64)> {
    let text = std::str::from_utf8(line)?;
    let mut fields = text.trim().split(',').map(str::trim);
    let mut next = || fields.next().context("Incomplete encoded packet header");
    ensure!(next()? == "0", "Unexpected encoded stream index");
    let dts = next()?.parse::<i64>()?;
    let pts = next()?.parse::<i64>()?;
    ensure!(next()?.parse::<i64>()? >= 0, "Invalid encoded duration");
    let size = next()?.parse::<usize>()?;
    ensure!(
        (1..=crate::video_queue::MAX_FRAME_BYTES).contains(&size),
        "Invalid encoded packet size"
    );
    let checksum = hex(next()?)?;
    // Optional flags and side-data sizes are diagnostics, never framing or
    // random-access authority. Annex B supplies the actual configuration/IDR.
    Ok((size, checksum, dts, pts))
}

pub(crate) fn adler(data: &[u8]) -> u32 {
    // framecrc uses Adler-32 with seed ZERO (not the usual seed one).
    let mut state = simd_adler32::Adler32::from_checksum(0);
    state.write(data);
    state.finish()
}

#[cfg(test)]
#[path = "packet_batch_profile.rs"]
mod batch_profile;
#[cfg(test)]
pub(crate) mod tests;
