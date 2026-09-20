//! T492: session-local sparse-stream admission, independent of native capture.
//! Callers supply matched encoded timestamps and render ACKs on one host clock.
//! A certificate is never persisted or transferred to another encoder epoch.

pub const COMPATIBLE_MS: u32 = 200;
pub const SPARSE_MS: u32 = 500;
const WINDOW: usize = 16;
const MAX_ACK_US: u64 = 100_000;
const COST_MARGIN_US: i128 = 20_000;

pub fn timestamp_us(value: i128, numerator: u32, denominator: u32) -> Option<i64> {
    if numerator == 0 || denominator == 0 {
        return None;
    }
    value
        .checked_mul(i128::from(numerator))?
        .checked_mul(1_000_000)?
        .checked_div(i128::from(denominator))?
        .try_into()
        .ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Baseline,
    Trial,
    Active,
    Rejected,
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub sequence: u32,
    pub pts_us: i64,
    pub ready_us: u64,
    pub ack_us: u64,
    pub keyframe: bool,
}

pub struct Policy {
    pub phase: Phase,
    previous: Option<Sample>,
    reference: Option<Sample>,
    last_key_us: Option<u64>,
    costs: Vec<i128>,
    baseline: i128,
    trial_start: u64,
}

impl Policy {
    pub fn new(current_named_decoder: bool) -> Self {
        Self {
            phase: if current_named_decoder {
                Phase::Baseline
            } else {
                Phase::Rejected
            },
            previous: None,
            reference: None,
            last_key_us: None,
            costs: Vec::with_capacity(WINDOW),
            baseline: 0,
            trial_start: 0,
        }
    }

    pub fn interval_ms(&self) -> u32 {
        if matches!(self.phase, Phase::Trial | Phase::Active) {
            SPARSE_MS
        } else {
            COMPATIBLE_MS
        }
    }

    pub fn reject(&mut self) {
        self.phase = Phase::Rejected;
        self.costs.clear();
    }

    pub fn tick(&mut self, now_us: u64) {
        if self.interval_ms() == SPARSE_MS
            && self
                .previous
                .is_some_and(|frame| now_us.saturating_sub(frame.ack_us) > 1_500_000)
        {
            self.reject();
        }
    }

    pub fn observe(&mut self, frame: Sample) {
        if self.phase == Phase::Rejected {
            return;
        }
        if !valid(frame, self.previous) {
            self.reject();
            return;
        }
        let gap = self.previous.map(|previous| frame.pts_us - previous.pts_us);
        let anchor = *self.reference.get_or_insert(frame);
        let cost = (i128::from(frame.ack_us) - i128::from(anchor.ready_us))
            - (i128::from(frame.pts_us) - i128::from(anchor.pts_us));
        self.previous = Some(frame);
        if !self.keyframe_progress(frame) {
            self.reject();
            return;
        }
        match self.phase {
            Phase::Baseline => self.baseline_sample(frame, gap, cost),
            Phase::Trial => self.trial_sample(frame, gap, cost),
            Phase::Active => self.active_sample(frame, cost),
            Phase::Rejected => unreachable!(),
        }
    }

    fn keyframe_progress(&mut self, frame: Sample) -> bool {
        let recent = self
            .last_key_us
            .is_none_or(|last| frame.ready_us.saturating_sub(last) <= 1_600_000);
        if frame.keyframe {
            self.last_key_us = Some(frame.ready_us);
        }
        self.phase == Phase::Baseline || recent
    }

    fn baseline_sample(&mut self, frame: Sample, gap: Option<i64>, cost: i128) {
        if !gap.is_some_and(|gap| (150_000..=300_000).contains(&gap)) {
            self.costs.clear();
            self.reference = Some(frame);
            return;
        }
        if frame.ack_us - frame.ready_us > MAX_ACK_US {
            self.costs.clear();
            return;
        }
        self.costs.push(cost);
        if self.costs.len() == WINDOW && self.last_key_us.is_some() {
            self.baseline = percentile(&self.costs);
            self.costs.clear();
            self.trial_start = frame.ack_us;
            self.phase = Phase::Trial;
        } else if self.costs.len() == WINDOW {
            self.costs.clear();
        }
    }

    fn trial_sample(&mut self, frame: Sample, gap: Option<i64>, cost: i128) {
        if !gap.is_some_and(|gap| (350_000..=750_000).contains(&gap)) {
            // Allow the pending compatible-cadence frame at the transition.
            if frame.ack_us.saturating_sub(self.trial_start) < 1_000_000 {
                return;
            }
            self.phase = Phase::Baseline;
            self.costs.clear();
            self.reference = Some(frame);
            return;
        }
        if !self.acceptable(frame, cost) {
            self.reject();
            return;
        }
        self.costs.push(cost);
        if self.costs.len() == WINDOW {
            self.costs.clear();
            self.phase = Phase::Active;
        }
    }

    fn acceptable(&self, frame: Sample, cost: i128) -> bool {
        frame.ack_us - frame.ready_us <= MAX_ACK_US
            && (cost - self.baseline).abs() <= COST_MARGIN_US
    }

    fn active_sample(&mut self, frame: Sample, cost: i128) {
        if !self.acceptable(frame, cost) {
            self.reject();
        }
    }
}

fn valid(frame: Sample, previous: Option<Sample>) -> bool {
    if frame.pts_us < 0 || frame.ack_us < frame.ready_us {
        return false;
    }
    previous.is_none_or(|previous| {
        frame.sequence == previous.sequence.wrapping_add(1)
            && frame.pts_us > previous.pts_us
            && frame.ready_us >= previous.ready_us
            && frame.ack_us >= previous.ack_us
    })
}

fn percentile(values: &[i128]) -> i128 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[((sorted.len() - 1) * 95).div_ceil(100)]
}

#[cfg(test)]
mod tests;
