//! Worker-owned bounded queue/resampler. Native callbacks use adapter-owned rings.
use super::{AudioProfile, PcmBlock, BLOCK_FRAMES, MAX_DRIFT_PPM};
use anyhow::{ensure, Result};
use std::collections::VecDeque;

const CAPACITY: usize = 20;
const SCALE: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderResult {
    pub silence: bool,
    pub discontinuity: bool,
}

pub struct PcmQueue {
    blocks: VecDeque<(u64, PcmBlock)>,
    channels: usize,
    target: usize,
    offset: usize,
    phase: u64,
    ppm: i32,
    primed: bool,
    discontinuity: bool,
    last_time: u64,
}

impl PcmQueue {
    pub fn new(profile: AudioProfile) -> Result<Self> {
        profile.validate()?;
        Ok(Self {
            blocks: VecDeque::with_capacity(CAPACITY),
            channels: profile.direction.channels(),
            target: profile.buffer_ms as usize / 10,
            offset: 0,
            phase: 0,
            ppm: 0,
            primed: false,
            discontinuity: false,
            last_time: 0,
        })
    }

    pub fn clear(&mut self) {
        self.blocks.clear();
        self.offset = 0;
        self.phase = 0;
        self.ppm = 0;
        self.primed = false;
        self.discontinuity = true;
    }

    pub fn queued_frames(&self) -> usize {
        self.blocks.len() * BLOCK_FRAMES - self.offset
    }

    fn advance_clock(&mut self, now_ms: u64) -> Result<()> {
        ensure!(now_ms >= self.last_time, "audio clock moved backwards");
        self.last_time = now_ms;
        while self.blocks.front().is_some_and(|(at, _)| now_ms - at > 200) {
            self.drop_oldest();
            self.primed = false;
        }
        Ok(())
    }

    fn drop_oldest(&mut self) {
        self.blocks.pop_front();
        self.offset = 0;
        self.phase = 0;
        self.discontinuity = true;
    }

    pub fn push(&mut self, block: PcmBlock, now_ms: u64) -> Result<()> {
        ensure!(
            block.channels == self.channels,
            "audio queue channel mismatch"
        );
        self.advance_clock(now_ms)?;
        if block.discontinuity {
            self.clear();
        }
        if self.blocks.len() == CAPACITY {
            self.drop_oldest();
        }
        self.blocks.push_back((now_ms, block));
        Ok(())
    }

    /// Ratio of native source/destination frame-counter deltas over the same
    /// observation window. Never subtract clocks belonging to different devices.
    /// Excess drift flushes audio; callers report/re-prime instead of hiding it.
    pub fn adjust_drift(&mut self, source_frames: u64, destination_frames: u64) -> Result<i32> {
        ensure!(
            source_frames > 0 && destination_frames > 0,
            "empty audio drift window"
        );
        let difference = (source_frames as i128 - destination_frames as i128) * SCALE as i128;
        if difference.abs() > MAX_DRIFT_PPM as i128 * destination_frames as i128 {
            self.clear();
            anyhow::bail!("audio clock drift exceeds 1000 ppm");
        }
        self.ppm = (difference / destination_frames as i128) as i32;
        Ok(self.ppm)
    }

    /// Exactly one output block, including stereo channel-aligned interpolation.
    /// No allocations, waits or IO. Partial underflow emits a whole silent block.
    pub fn render(&mut self, now_ms: u64, output: &mut [i16]) -> Result<RenderResult> {
        output.fill(0);
        ensure!(
            output.len() == BLOCK_FRAMES * self.channels,
            "invalid audio output buffer"
        );
        self.advance_clock(now_ms)?;
        if !self.primed {
            self.primed = self.blocks.len() >= self.target;
        }
        let step = (SCALE as i64 + self.ppm as i64) as u64;
        let end = self.phase + BLOCK_FRAMES as u64 * step;
        let last = self.phase + (BLOCK_FRAMES - 1) as u64 * step;
        let needed = last.div_ceil(SCALE) as usize + 1;
        let silence = !self.primed || self.queued_frames() < needed.max((end / SCALE) as usize);
        if silence {
            self.underflow();
        } else {
            self.interpolate(output, step);
            self.consume((end / SCALE) as usize);
            self.phase = end % SCALE;
        }
        Ok(RenderResult {
            silence,
            discontinuity: std::mem::take(&mut self.discontinuity),
        })
    }

    fn underflow(&mut self) {
        if self.primed {
            self.clear();
        }
    }

    fn sample(&self, frame: usize, channel: usize) -> i16 {
        let frame = self.offset + frame;
        self.blocks[frame / BLOCK_FRAMES].1.samples
            [(frame % BLOCK_FRAMES) * self.channels + channel]
    }

    fn interpolate(&self, output: &mut [i16], step: u64) {
        for (frame, channels) in output.chunks_exact_mut(self.channels).enumerate() {
            let position = self.phase + frame as u64 * step;
            let index = (position / SCALE) as usize;
            let fraction = (position % SCALE) as i64;
            for (channel, value) in channels.iter_mut().enumerate() {
                let first = self.sample(index, channel) as i64;
                let next = self.sample(index + usize::from(fraction != 0), channel) as i64;
                *value = (first + (next - first) * fraction / SCALE as i64) as i16;
            }
        }
    }

    fn consume(&mut self, frames: usize) {
        self.offset += frames;
        while self.offset >= BLOCK_FRAMES {
            self.blocks.pop_front();
            self.offset -= BLOCK_FRAMES;
        }
    }
}
