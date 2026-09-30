//! Device-counter rates use each endpoint's own monotonic elapsed time.
//! Absolute clocks and packet arrival rates are never compared.
use anyhow::{ensure, Result};

pub const CLOCK_BYTES: usize = 24;
const WINDOW_NS: u64 = 2_000_000_000;
const MAX_WINDOW_NS: u64 = 5_000_000_000;
const FRESH_MS: u64 = 3_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockSample {
    pub epoch: u64,
    pub frames: u64,
    pub nanos: u64,
}
impl ClockSample {
    pub fn valid(self) -> bool {
        self.epoch > 0
            && self.epoch <= i64::MAX as u64
            && self.frames <= i64::MAX as u64
            && self.nanos > 0
            && self.nanos <= i64::MAX as u64
    }
    pub fn decode(bytes: &[u8]) -> Result<Option<Self>> {
        ensure!(
            bytes.len() == CLOCK_BYTES,
            "invalid native audio clock length"
        );
        if bytes.iter().all(|byte| *byte == 0) {
            return Ok(None);
        }
        let sample = Self {
            epoch: u64::from_be_bytes(bytes[..8].try_into()?),
            frames: u64::from_be_bytes(bytes[8..16].try_into()?),
            nanos: u64::from_be_bytes(bytes[16..24].try_into()?),
        };
        ensure!(sample.valid(), "invalid native audio clock");
        Ok(Some(sample))
    }
    pub fn encode(sample: Option<Self>) -> Result<[u8; CLOCK_BYTES]> {
        let mut bytes = [0; CLOCK_BYTES];
        if let Some(sample) = sample {
            ensure!(sample.valid(), "invalid native audio clock");
            bytes[..8].copy_from_slice(&sample.epoch.to_be_bytes());
            bytes[8..16].copy_from_slice(&sample.frames.to_be_bytes());
            bytes[16..].copy_from_slice(&sample.nanos.to_be_bytes());
        }
        Ok(bytes)
    }
}

#[derive(Default)]
pub(super) struct Window {
    base: Option<ClockSample>,
    last: Option<ClockSample>,
    completed_ms: u64,
    rate: Option<(u64, u64)>,
}
impl Window {
    /// True requests a discontinuity. Missing counters disable correction;
    /// duplicate native snapshots neither invent progress nor refresh evidence.
    pub(super) fn observe(&mut self, sample: Option<ClockSample>, now_ms: u64) -> bool {
        let Some(sample) = sample.filter(|sample| sample.valid()) else {
            *self = Self::default();
            return false;
        };
        if self.last == Some(sample) {
            return false;
        }
        let reset = self.last.is_some_and(|last| {
            sample.epoch != last.epoch || sample.frames < last.frames || sample.nanos <= last.nanos
        });
        if reset {
            *self = Self::default();
        }
        self.last = Some(sample);
        let base = *self.base.get_or_insert(sample);
        reset || self.complete(base, sample, now_ms)
    }
    fn complete(&mut self, base: ClockSample, sample: ClockSample, now_ms: u64) -> bool {
        let nanos = sample.nanos - base.nanos;
        if nanos < WINDOW_NS {
            return false;
        }
        self.base = Some(sample);
        let frames = sample.frames - base.frames;
        // Bound normalized products and reject stalls/unphysical counters.
        if nanos > MAX_WINDOW_NS || frames == 0 || frames > 480_000 {
            self.rate = None;
            return true;
        }
        self.rate = Some((frames, nanos));
        self.completed_ms = now_ms;
        false
    }
    fn rate(&self, now_ms: u64) -> Option<(u64, u64)> {
        (now_ms.checked_sub(self.completed_ms)? <= FRESH_MS)
            .then_some(self.rate)
            .flatten()
    }
}

#[derive(Default)]
pub(super) struct Drift {
    pub(super) source: Window,
    pub(super) destination: Window,
}
impl Drift {
    /// Normalize both native windows to the same duration by cross multiplying.
    /// Products are bounded by Window::complete; no absolute-clock subtraction.
    pub(super) fn ratio(&self, now_ms: u64) -> Option<(u64, u64)> {
        let (source, source_ns) = self.source.rate(now_ms)?;
        let (destination, destination_ns) = self.destination.rate(now_ms)?;
        Some((source * destination_ns, destination * source_ns))
    }
}

#[cfg(test)]
mod tests;
