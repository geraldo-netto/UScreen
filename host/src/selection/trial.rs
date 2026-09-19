//! Bounded, generation-specific packet-ready → render-ACK observations.
use super::Key;
use crate::latency::RenderSample;
use crate::latency::{EncoderEvidence, LatencyTracker};
use std::{future::Future, time::Duration};
use tokio::sync::watch;
use uscreen_config::negotiation::DecoderChoice;

/// Each candidate receives its own bounded window. Never merge encoder epochs.
pub(super) async fn observe(
    tracker: &LatencyTracker,
    key: &Key,
    encoder: &str,
    decoder: Option<&DecoderChoice>,
) -> Option<Observation> {
    let started = tokio::time::Instant::now();
    let before = tracker.encoder_evidence().map(|e| (e.epoch, e.rendered()));
    let mut updates = tracker.activity_updates();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if let Some(result) = tracker
                .encoder_evidence()
                .and_then(|e| observations(&e, key, encoder, decoder, before, started))
            {
                return Some(result);
            }
            if updates.changed().await.is_err() {
                return None;
            }
        }
    })
    .await
    .ok()
    .flatten()
}

fn observations(
    evidence: &EncoderEvidence,
    key: &Key,
    encoder: &str,
    decoder: Option<&DecoderChoice>,
    before: Option<(u64, u64)>,
    started: tokio::time::Instant,
) -> Option<Observation> {
    if !evidence.active()
        || evidence.name != encoder
        || evidence.format != key.format
        || evidence.decoder.as_ref() != decoder
    {
        return None;
    }
    let floor = before
        .filter(|&(epoch, _)| epoch == evidence.epoch)
        .map_or(0, |(_, count)| count);
    let samples = evidence.samples_after(floor);
    let first = samples.first()?;
    if first.ordinal != floor + 1 {
        return None;
    }
    let mut result = Observation::from_samples(samples.get(3..)?, started)?;
    result.startup_us = first
        .at
        .saturating_duration_since(started)
        .as_micros()
        .min(u64::MAX as u128) as u64;
    Some(result)
}

pub(super) async fn quiet(updates: &mut watch::Receiver<Option<tokio::time::Instant>>) -> bool {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let last = *updates.borrow_and_update();
            let Some(last) = last else {
                return true;
            };
            tokio::select! {
                _ = tokio::time::sleep_until(last + Duration::from_secs(2)) => return true,
                result = updates.changed() => if result.is_err() { return false; },
            }
        }
    })
    .await
    .unwrap_or(false)
}

pub(super) async fn uninterrupted<T>(
    input: &mut watch::Receiver<Option<tokio::time::Instant>>,
    work: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = input.changed() => None,
        result = work => Some(result),
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct Observation {
    pub p50_us: u32,
    pub p95_us: u32,
    pub p99_us: u32,
    pub ack_fps: f64,
    pub delivery_permille: u64,
    pub startup_us: u64,
    pub samples: usize,
}

impl Observation {
    pub fn from_samples(samples: &[RenderSample], started: tokio::time::Instant) -> Option<Self> {
        let first = samples.first()?;
        let last = samples.last()?;
        let duration = last.at.saturating_duration_since(first.at);
        let output = last.output.saturating_sub(first.output).max(1);
        let delivery = (samples.len() - 1) as u64 * 1000 / output;
        if samples.len() < 12 || duration.as_secs_f64() < 2.0 || delivery < 900 {
            return None;
        }
        let mut times: Vec<_> = samples.iter().map(|s| s.micros).collect();
        times.sort_unstable();
        let percentile = |percent: usize| times[((times.len() - 1) * percent).div_ceil(100)];
        Some(Self {
            p50_us: percentile(50),
            p95_us: percentile(95),
            p99_us: percentile(99),
            ack_fps: (samples.len() - 1) as f64 / duration.as_secs_f64(),
            delivery_permille: delivery.min(1000),
            samples: samples.len(),
            startup_us: first
                .at
                .saturating_duration_since(started)
                .as_micros()
                .min(u64::MAX as u128) as u64,
        })
    }

    pub fn improves(&self, previous: &Self) -> bool {
        let margin = (previous.p95_us / 10).max(2_000);
        self.comparable(previous) && self.p95_us.saturating_add(margin) < previous.p95_us
    }

    pub fn similar_speed(&self, previous: &Self) -> bool {
        self.comparable(previous) && self.p95_us.abs_diff(previous.p95_us) <= 2_000
    }

    fn comparable(&self, previous: &Self) -> bool {
        (0.8..=1.25).contains(&(self.ack_fps / previous.ack_fps))
            && self.p99_us <= previous.p99_us.saturating_add(2_000)
            && self.startup_us <= previous.startup_us.saturating_add(2_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn acknowledge(
        tracker: &LatencyTracker,
        evidence: &std::sync::Arc<EncoderEvidence>,
        count: usize,
    ) {
        for _ in 0..count {
            let seq = tracker.next_sequence();
            tracker.on_encoded_for(seq, evidence);
            tracker.on_rendered(seq, 100);
            tokio::time::advance(Duration::from_millis(200)).await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn t479_restarted_generation_needs_its_own_warmup_and_window() {
        let tracker = LatencyTracker::new();
        let key = Key {
            format: (640, 480, 60, 20000, 18),
            epoch: 1,
            decoders: None,
        };
        let worker = tokio::spawn({
            let tracker = tracker.clone();
            let key = key.clone();
            async move { observe(&tracker, &key, "libx264", None).await }
        });
        tokio::task::yield_now().await;
        let old = tracker.encoder_started("libx264", key.format);
        acknowledge(&tracker, &old, 5).await;
        let current = tracker.encoder_started("libx264", key.format);
        acknowledge(&tracker, &current, 10).await;
        assert!(
            !worker.is_finished(),
            "T479: merged retired samples or omitted warmup"
        );
        acknowledge(&tracker, &current, 6).await;
        let observed = worker
            .await
            .unwrap()
            .expect("matching generation must complete");
        assert!(observed.samples >= 12);
        assert_eq!(observed.ack_fps, 5.0);
        assert_eq!(
            observed.startup_us, 1_000_000,
            "first matching ACK, not the post-warmup sample"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn t479_tablet_interaction_cancels_work_and_quiet_wait_is_bounded() {
        let tracker = LatencyTracker::new();
        let mut input = tracker.interaction_updates();
        let pending = uninterrupted(&mut input, async {
            tracker.note_interaction();
            std::future::pending::<()>().await;
        });
        assert!(pending.await.is_none());
        assert!(quiet(&mut input).await);
        let busy = tokio::spawn({
            let tracker = tracker.clone();
            async move {
                loop {
                    tracker.note_interaction();
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        });
        tokio::task::yield_now().await;
        let started = tokio::time::Instant::now();
        assert!(!quiet(&mut input).await);
        assert_eq!(started.elapsed(), Duration::from_secs(10));
        busy.abort();
        let _ = busy.await;
    }
    fn samples(latency: u32, spacing: u64, count: u64) -> Vec<RenderSample> {
        let now = tokio::time::Instant::now();
        (0..count)
            .map(|index| RenderSample {
                ordinal: index + 4,
                output: index + 4,
                at: now + Duration::from_millis(index * spacing),
                micros: latency,
            })
            .collect()
    }
    #[test]
    fn t479_measured_tail_improves_over_a_host_only_winner() {
        let started = tokio::time::Instant::now();
        let high = Observation::from_samples(&samples(30_000, 50, 41), started).unwrap();
        let baseline = Observation::from_samples(&samples(12_000, 50, 41), started).unwrap();
        assert!(baseline.improves(&high));
        assert!(!high.improves(&baseline));
        assert_eq!(baseline.p95_us, 12_000);
        assert_eq!(baseline.samples, 41);
        assert_eq!(baseline.delivery_permille, 1000);
    }
    #[test]
    fn t479_sparse_stalled_dropped_and_too_short_trials_are_not_speed_proof() {
        let started = tokio::time::Instant::now();
        assert!(Observation::from_samples(&[], started).is_none());
        assert!(Observation::from_samples(&samples(10, 1, 50), started).is_none());
        assert!(Observation::from_samples(&samples(10, 500, 8), started).is_none());
        let mut dropped = samples(10, 50, 41);
        for sample in &mut dropped {
            sample.output *= 2;
        }
        assert!(Observation::from_samples(&dropped, started).is_none());
        assert!(Observation::from_samples(&samples(12_000, 200, 16), started).is_some());
    }
    #[test]
    fn t479_noise_unmatched_rates_and_worse_tail_do_not_displace_incumbent() {
        let started = tokio::time::Instant::now();
        let previous = Observation::from_samples(&samples(12_000, 50, 41), started).unwrap();
        let noise = Observation {
            p95_us: 11_000,
            ..previous.clone()
        };
        let different_load = Observation {
            p95_us: 5_000,
            ack_fps: 3.0,
            ..previous.clone()
        };
        let bad_tail = Observation {
            p95_us: 5_000,
            p99_us: 90_000,
            ..previous.clone()
        };
        assert!(!noise.improves(&previous));
        assert!(!different_load.improves(&previous));
        assert!(!bad_tail.improves(&previous));
    }
}
