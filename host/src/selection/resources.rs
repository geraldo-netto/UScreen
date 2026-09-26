//! Bounded resource comparisons; OS counters enter through an explicit sampler.
use serde::{Deserialize, Serialize};
use std::time::Duration;
pub(super) mod linux;

pub(super) struct Counter {
    pub identity: u64,
    pub cpu_us: u64,
    pub rss_bytes: u64,
}

pub(super) trait Sampler {
    fn sample(&self, pid: u32) -> Option<Counter>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Usage {
    pub cpu_us_per_frame: f64,
    pub peak_rss_bytes: u64,
}
impl Usage {
    pub fn valid(&self) -> bool {
        self.cpu_us_per_frame.is_finite() && self.cpu_us_per_frame >= 0.0
            && self.peak_rss_bytes > 0
    }
    pub fn acceptable(&self, baseline: &Self) -> bool {
        self.valid() && baseline.valid()
            && self.cpu_us_per_frame <= baseline.cpu_us_per_frame * 1.10
            && self.peak_rss_bytes as f64 <= baseline.peak_rss_bytes as f64 * 1.20
    }
}

#[derive(Default)]
pub(super) struct Window {
    epoch: u64,
    first: Option<(Counter, u64, Duration)>,
    last: Option<(Counter, u64)>,
    sampled: Duration,
    peak: u64,
    count: usize,
    invalid: bool,
}
impl Window {
    pub fn poll(&mut self, epoch: u64, pid: Option<u32>, frames: u64, elapsed: Duration, source: &impl Sampler) {
        if self.epoch != epoch {
            *self = Self { epoch, ..Self::default() };
        }
        if elapsed.saturating_sub(self.sampled) < Duration::from_millis(100) { return; }
        self.sampled = elapsed;
        let Some(counter) = pid.and_then(|pid| source.sample(pid)) else { self.invalid = true; return; };
        self.peak = self.peak.max(counter.rss_bytes);
        self.count += 1;
        if self.first.is_none() { self.first = Some((counter, frames, elapsed)); }
        else { self.last = Some((counter, frames)); }
    }
    pub fn finish(&self) -> Option<Usage> {
        let (first, frames, at) = self.first.as_ref()?;
        let (last, end) = self.last.as_ref()?;
        if self.invalid || self.count < 8 || first.identity != last.identity
            || self.sampled.saturating_sub(*at) < Duration::from_secs(1) { return None; }
        let count = end.checked_sub(*frames).filter(|v| *v >= 12)?;
        Some(Usage {
            cpu_us_per_frame: last.cpu_us.checked_sub(first.cpu_us)? as f64 / count as f64,
            peak_rss_bytes: self.peak,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixed(std::cell::Cell<u64>);
    impl Sampler for Fixed {
        fn sample(&self, _: u32) -> Option<Counter> {
            self.0.set(self.0.get() + 100);
            Some(Counter { identity: 7, cpu_us: self.0.get(), rss_bytes: 1024 })
        }
    }
    #[test]
    fn t612_resources_are_generation_bound_and_unknown_never_zero() {
        let source = Fixed(0.into());
        let mut window = Window::default();
        assert!(window.finish().is_none());
        for i in 1..=20 { window.poll(1, Some(2), i * 2, Duration::from_millis(i * 100), &source); }
        let usage = window.finish().unwrap();
        assert_eq!(usage.cpu_us_per_frame, 50.0);
        assert_eq!(usage.peak_rss_bytes, 1024);
        assert!(usage.acceptable(&usage));
        assert!(!Usage { cpu_us_per_frame: 100.0, ..usage.clone() }.acceptable(&usage));
        window.poll(1, None, 42, Duration::from_millis(2100), &source);
        assert!(window.finish().is_none());
        window.poll(2, Some(2), 1, Duration::from_millis(2200), &source);
        window.poll(2, Some(2), 1, Duration::from_millis(2201), &source);
        assert!(window.finish().is_none());
        assert!(!Usage { cpu_us_per_frame: f64::NAN, peak_rss_bytes: 1 }.valid());
    }
}
