//! T695: cadence requests belong to one encoder/decoder/viewer epoch.
//! Adapters apply requests only to unchanged frames, preserve motion and IDRs,
//! expire requests after 1.5 seconds, and clear only their owned request.
use super::{Phase, Policy, Sample, COMPATIBLE_MS};
use anyhow::Result;
use std::collections::VecDeque;

pub const LEASE_US: u64 = 1_500_000;
const MAX_PENDING: usize = 256;

pub trait Cadence {
    fn publish(&self, interval_ms: u32) -> Result<()>;
    fn clear(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Epoch {
    pub encoder: u64,
    pub decoder: u64,
    pub viewer: u64,
}
impl Epoch {
    fn valid(self) -> bool {
        self.encoder != 0 && self.decoder != 0 && self.viewer != 0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Encoded {
    pub sequence: u32,
    pub pts_us: Option<i64>,
    pub ready_us: u64,
    pub keyframe: bool,
}

/// Bounded timing evidence; missing, reordered or unmatched ACKs fail closed.
pub struct Control<A: Cadence> {
    adapter: A,
    epoch: Epoch,
    policy: Policy,
    pending: VecDeque<Encoded>,
    ordinal: u64,
}
impl<A: Cadence> Control<A> {
    pub fn new(adapter: A, epoch: Epoch, named_decoder: bool) -> Self {
        adapter.clear();
        Self {
            adapter,
            epoch,
            policy: Policy::new(named_decoder && epoch.valid()),
            pending: VecDeque::new(),
            ordinal: 0,
        }
    }
    pub fn phase(&self) -> Phase {
        self.policy.phase
    }
    pub fn encoded(&mut self, epoch: Epoch, frame: Encoded) {
        if epoch != self.epoch {
            self.reject();
            return;
        }
        self.pending.push_back(frame);
        if self.pending.len() > MAX_PENDING {
            self.pending.pop_front();
        }
    }
    pub fn acknowledge(&mut self, epoch: Epoch, ordinal: u64, sequence: u32, at_us: u64) {
        if epoch != self.epoch || self.ordinal.checked_add(1) != Some(ordinal) {
            self.reject();
            return;
        }
        self.ordinal = ordinal;
        match self.take(sequence, at_us) {
            Some(sample) => self.policy.observe(sample),
            None => self.reject(),
        }
        if self.policy.phase == Phase::Rejected {
            self.adapter.clear();
        }
    }
    fn take(&mut self, sequence: u32, ack_us: u64) -> Option<Sample> {
        let position = self.pending.iter().position(|f| f.sequence == sequence)?;
        self.pending.drain(..position);
        let frame = self.pending.pop_front()?;
        Some(Sample {
            sequence,
            pts_us: frame.pts_us?,
            ready_us: frame.ready_us,
            ack_us,
            keyframe: frame.keyframe,
        })
    }
    /// Caller supplies one monotonic clock and the current live epoch.
    pub fn tick(&mut self, epoch: Epoch, active: bool, now_us: u64) -> Result<()> {
        if epoch != self.epoch || !active {
            self.reject();
        }
        self.policy.tick(now_us);
        let interval = self.policy.interval_ms();
        if interval == COMPATIBLE_MS {
            self.adapter.clear();
            return Ok(());
        }
        if let Err(error) = self.adapter.publish(interval) {
            self.reject();
            return Err(error);
        }
        Ok(())
    }
    fn reject(&mut self) {
        self.policy.reject();
        self.pending.clear();
        self.adapter.clear();
    }
}
impl<A: Cadence> Drop for Control<A> {
    fn drop(&mut self) {
        self.adapter.clear();
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, windows))]
mod windows_tests;
