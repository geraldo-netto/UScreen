//! Encoded-packet-to-render-acknowledgement latency on the host clock.
//!
//! The timer starts when a complete access unit is ready for broadcast, after
//! encoding and packetizer buffering. It ends when the tablet's render-callback
//! acknowledgement reaches the host, so it includes the reverse message path
//! and callback scheduling. The legacy log label is `encode→display`.
//!
//! The helper's separate capture→FIFO timer starts after the EVDI grab. Neither
//! metric covers the compositor wait, grab, encoding, or packetizer assembly;
//! their percentiles cannot be summed into total display latency.

use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex,
};
use std::time::Instant;
use tracing::info;

/// How many in-flight frames to remember. At 60 fps this is ~4s of history,
/// far more than any sane round trip, and bounded so a tablet that stops
/// reporting cannot grow this without limit.
const MAX_TRACKED: usize = 256;

/// Samples kept for the percentile report. One report covers ~5s.
const MAX_SAMPLES: usize = 1024;

#[derive(Default)]
struct Inner {
    /// (seq, time the complete access unit became ready for broadcast), oldest first.
    sent: VecDeque<(u32, Instant)>,
    /// Round-trip latencies in microseconds, for the current report window.
    samples: Vec<u32>,
    /// Of that round trip, the part the tablet spent decoding and rendering.
    /// The remainder also includes host queueing and the return message path;
    /// subtracting independent medians is only a rough transport estimate.
    decode_samples: Vec<u32>,
    /// Frames that were sent but whose acknowledgement never arrived before
    /// they aged out — a direct sign of frames being dropped downstream.
    lost: u64,
    last_report: Option<Instant>,
}

#[derive(Clone, Default)]
pub struct LatencyTracker {
    inner: Arc<Mutex<Inner>>,
    next_sequence: Arc<AtomicU32>,
}

impl LatencyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shared by every encoder generation in this daemon instance.
    pub fn next_sequence(&self) -> u32 {
        self.next_sequence.fetch_add(1, Ordering::Relaxed)
    }

    /// A complete encoded access unit is ready for broadcast.
    pub fn on_encoded(&self, seq: u32) {
        let Ok(mut g) = self.inner.lock() else { return };
        if g.last_report.is_none() {
            g.last_report = Some(Instant::now());
        }
        g.sent.push_back((seq, Instant::now()));
        while g.sent.len() > MAX_TRACKED {
            g.sent.pop_front();
            g.lost += 1;
        }
    }

    /// The host received the tablet's render-callback acknowledgement.
    pub fn on_rendered(&self, seq: u32, decode_us: i64) {
        let Ok(mut g) = self.inner.lock() else { return };
        // Everything queued before this frame is now known to be behind it;
        // drop it so the deque tracks only genuinely in-flight frames.
        let Some(pos) = g.sent.iter().position(|(s, _)| *s == seq) else {
            return;
        };
        let (_, at) = g.sent[pos];
        let micros = at.elapsed().as_micros().min(u32::MAX as u128) as u32;
        g.sent.drain(..=pos);
        if g.samples.len() < MAX_SAMPLES {
            g.samples.push(micros);
            if decode_us > 0 {
                g.decode_samples
                    .push((decode_us as u128).min(u32::MAX as u128) as u32);
            }
        }
    }

    /// Log percentiles if the report window has elapsed. Cheap to call often.
    pub fn maybe_report(&self) {
        let Ok(mut g) = self.inner.lock() else { return };
        let Some(last) = g.last_report else { return };
        if last.elapsed().as_secs() < 5 {
            return;
        }
        g.last_report = Some(Instant::now());

        if g.samples.is_empty() {
            // Silence here means the tablet never acknowledged anything, which
            // is itself worth saying out loud.
            if g.lost > 0 {
                info!(
                    "Latency: no frames acknowledged by the tablet ({} aged out)",
                    g.lost
                );
                g.lost = 0;
            }
            return;
        }

        let mut s = std::mem::take(&mut g.samples);
        let mut d = std::mem::take(&mut g.decode_samples);
        let lost = std::mem::take(&mut g.lost);
        let inflight = g.sent.len();
        drop(g);

        s.sort_unstable();
        let pct = |v: &[u32], p: f64| -> f64 {
            let idx = ((v.len() as f64 - 1.0) * p).round() as usize;
            v[idx] as f64 / 1000.0
        };
        let total_p50 = pct(&s, 0.50);
        info!(
            "Latency encode→display: p50 {:.1}ms  p95 {:.1}ms  max {:.1}ms  ({} samples, {} in flight, {} aged out)",
            total_p50,
            pct(&s, 0.95),
            pct(&s, 1.0),
            s.len(),
            inflight,
            lost
        );

        // The split is what tells us where to spend effort: a large decode
        // share means a different transport would buy nothing.
        if !d.is_empty() {
            d.sort_unstable();
            let decode_p50 = pct(&d, 0.50);
            info!(
                "  of which tablet decode+render p50 {:.1}ms  p95 {:.1}ms  → wire ~{:.1}ms",
                decode_p50,
                pct(&d, 0.95),
                (total_p50 - decode_p50).max(0.0)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t228_retired_ack_cannot_consume_new_generation() {
        let tracker = LatencyTracker::new();
        let old = tracker.next_sequence();
        tracker.on_encoded(old);
        tracker.inner.lock().unwrap().sent[0].1 -= std::time::Duration::from_secs(2);
        let restarted = tracker.clone();
        let fresh = restarted.next_sequence();
        restarted.on_encoded(fresh);
        tracker.on_rendered(old, 0);
        assert_eq!(tracker.inner.lock().unwrap().sent[0].0, fresh);
        tracker.on_rendered(fresh, 0);
        tracker.on_rendered(old, 0);
        let state = tracker.inner.lock().unwrap();
        assert!(state.sent.is_empty());
        assert_eq!(state.samples.len(), 2);
        assert!(state.samples[0] >= 2_000_000);
        assert!(state.samples[1] < 1_000_000);
    }
}
