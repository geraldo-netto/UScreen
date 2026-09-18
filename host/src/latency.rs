//! Encoded-packet-to-render-acknowledgement latency on the host clock.
//!
//! The timer starts when a complete access unit is ready for broadcast, after
//! encoding and packetizer buffering. It ends when the tablet's render-callback
//! acknowledgement reaches the host, so it includes the reverse message path
//! and callback scheduling. This is not optical display latency.
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
    sent: VecDeque<(u32, Instant, Option<Arc<EncoderEvidence>>)>,
    encoder: Option<Arc<EncoderEvidence>>,
    encoder_epoch: u64,
    discontinuous: bool,
    /// Round-trip latencies in microseconds, for the current report window.
    samples: Vec<u32>,
    /// Complete-frame arrival to render-callback execution on the tablet clock.
    /// Missing tablet durations can produce a different sample population;
    /// independent percentiles cannot be subtracted to measure network latency.
    decode_samples: Vec<u32>,
    spare_samples: Vec<u32>,
    spare_decode_samples: Vec<u32>,
    /// Frames that were sent but whose acknowledgement never arrived before
    /// they aged out — a direct sign of frames being dropped downstream.
    lost: u64,
    last_report: Option<Instant>,
}

#[derive(Clone)]
pub struct LatencyTracker {
    inner: Arc<Mutex<Inner>>,
    next_sequence: Arc<AtomicU32>,
    activity: tokio::sync::watch::Sender<()>,
}

impl Default for LatencyTracker {
    fn default() -> Self {
        Self {
            inner: Default::default(),
            next_sequence: Default::default(),
            activity: tokio::sync::watch::channel(()).0,
        }
    }
}

#[cfg(not(feature = "inproc-encoder"))]
pub(crate) struct EncoderActivity {
    encoder: Arc<EncoderEvidence>,
    activity: tokio::sync::watch::Sender<()>,
}
#[cfg(not(feature = "inproc-encoder"))]
impl Drop for EncoderActivity {
    fn drop(&mut self) {
        self.encoder.active.store(false, Ordering::Release);
        self.activity.send_replace(());
    }
}

/// Acknowledgements stay tied to the encoder that produced the sequence.
/// Delayed acknowledgements from a retired encoder cannot certify its successor.
#[cfg_attr(feature = "inproc-encoder", allow(dead_code))]
pub(crate) struct EncoderEvidence {
    pub name: String,
    pub format: (u32, u32, u32, u32, u32),
    pub epoch: u64,
    pub decoder: Option<uscreen_config::negotiation::DecoderChoice>,
    decoder_receipt: Option<String>,
    rendered: std::sync::atomic::AtomicU64,
    encoded: std::sync::atomic::AtomicU64,
    encoded_at_ack: std::sync::atomic::AtomicU64,
    active: std::sync::atomic::AtomicBool,
}
#[cfg_attr(feature = "inproc-encoder", allow(dead_code))]
impl EncoderEvidence {
    pub fn encoded(&self) -> u64 {
        self.encoded.load(Ordering::Relaxed)
    }
    pub fn unrendered_output(&self) -> u64 {
        self.encoded()
            .saturating_sub(self.encoded_at_ack.load(Ordering::Relaxed))
    }
    pub fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub fn rendered(&self) -> u64 {
        self.rendered.load(Ordering::Acquire)
    }
}

impl LatencyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shared by every encoder generation in this daemon instance.
    pub fn next_sequence(&self) -> u32 {
        self.next_sequence.fetch_add(1, Ordering::Relaxed)
    }

    #[cfg(any(test, feature = "inproc-encoder"))]
    pub fn encoder_started(
        &self,
        name: &str,
        format: (u32, u32, u32, u32, u32),
    ) -> Arc<EncoderEvidence> {
        self.encoder_started_with_decoder(name, format, None)
    }

    pub fn encoder_started_with_decoder(
        &self,
        name: &str,
        format: (u32, u32, u32, u32, u32),
        decoder: Option<uscreen_config::negotiation::DecoderChoice>,
    ) -> Arc<EncoderEvidence> {
        let mut state = self.inner.lock().unwrap();
        state.encoder_epoch = state.encoder_epoch.wrapping_add(1);
        let evidence = Arc::new(EncoderEvidence {
            name: name.into(),
            format,
            epoch: state.encoder_epoch,
            decoder_receipt: decoder.as_ref().map(|d| d.receipt()),
            decoder,
            rendered: Default::default(),
            encoded: Default::default(),
            encoded_at_ack: Default::default(),
            active: std::sync::atomic::AtomicBool::new(true),
        });
        state.encoder = Some(evidence.clone());
        drop(state);
        self.activity.send_replace(());
        evidence
    }
    #[cfg(not(feature = "inproc-encoder"))]
    pub fn encoder_evidence(&self) -> Option<Arc<EncoderEvidence>> {
        self.inner.lock().ok()?.encoder.clone()
    }

    #[cfg(not(feature = "inproc-encoder"))]
    pub fn encoder_activity(&self, encoder: Arc<EncoderEvidence>) -> EncoderActivity {
        EncoderActivity {
            encoder,
            activity: self.activity.clone(),
        }
    }
    #[cfg(not(feature = "inproc-encoder"))]
    pub fn activity_updates(&self) -> tokio::sync::watch::Receiver<()> {
        self.activity.subscribe()
    }

    /// A complete encoded access unit is ready for broadcast.
    #[cfg(test)]
    pub fn on_encoded(&self, seq: u32) {
        self.record_encoded(seq, None);
    }
    pub fn on_encoded_for(&self, seq: u32, encoder: &Arc<EncoderEvidence>) {
        encoder.encoded.fetch_add(1, Ordering::Relaxed);
        self.record_encoded(seq, Some(encoder.clone()));
        self.activity.send_replace(());
    }
    #[cfg(not(feature = "inproc-encoder"))]
    pub fn on_encoder_output(&self, encoder: &Arc<EncoderEvidence>) {
        encoder.encoded.fetch_add(1, Ordering::Relaxed);
        self.activity.send_replace(());
    }
    fn record_encoded(&self, seq: u32, encoder: Option<Arc<EncoderEvidence>>) {
        let Ok(mut g) = self.inner.lock() else { return };
        if g.last_report.is_none() {
            g.last_report = Some(Instant::now());
        }
        if g.sent
            .back()
            .is_some_and(|(previous, _, _)| previous.wrapping_add(1) != seq)
        {
            g.discontinuous = true;
        }
        g.sent.push_back((seq, Instant::now(), encoder));
        while g.sent.len() > MAX_TRACKED {
            g.sent.pop_front();
            g.lost += 1;
        }
    }

    /// The host received the tablet's render-callback acknowledgement.
    #[cfg(test)]
    pub fn on_rendered(&self, seq: u32, decode_us: i64) {
        self.on_rendered_from(seq, decode_us, None);
    }

    pub fn on_rendered_from(&self, seq: u32, decode_us: i64, decoder: Option<&str>) {
        let Ok(mut g) = self.inner.lock() else { return };
        let Some(micros) = g.acknowledge(seq, decoder) else {
            return;
        };
        if g.samples.len() < MAX_SAMPLES {
            g.samples.push(micros);
            if decode_us > 0 {
                g.decode_samples
                    .push((decode_us as u128).min(u32::MAX as u128) as u32);
            }
        }
        drop(g);
        self.activity.send_replace(());
    }

    /// Log percentiles if the report window has elapsed. Cheap to call often.
    pub fn maybe_report(&self) {
        let Ok(mut g) = self.inner.lock() else { return };
        let Some(last) = g.last_report else { return };
        if last.elapsed().as_secs() < 5 {
            return;
        }
        g.last_report = Some(Instant::now());

        let mut report = g.take_report();
        drop(g);
        report.log();
        self.recycle(report);
    }

    fn recycle(&self, mut report: Report) {
        report.samples.clear();
        report.decode_samples.clear();
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        keep_capacity(&mut state.spare_samples, report.samples);
        keep_capacity(&mut state.spare_decode_samples, report.decode_samples);
    }
}

impl Inner {
    fn acknowledge(&mut self, seq: u32, decoder: Option<&str>) -> Option<u32> {
        // Everything queued before this frame is now known to be behind it;
        // drop it so the deque tracks only genuinely in-flight frames.
        let pos = self.position(seq)?;
        let (_, at, encoder) = &self.sent[pos];
        if encoder
            .as_ref()
            .is_some_and(|e| e.decoder_receipt.as_deref() != decoder)
        {
            return None;
        }
        if let Some(encoder) = encoder {
            encoder
                .encoded_at_ack
                .store(encoder.encoded(), Ordering::Relaxed);
            encoder.rendered.fetch_add(1, Ordering::Release);
        }
        let micros = at.elapsed().as_micros().min(u32::MAX as u128) as u32;
        self.sent.drain(..=pos);
        if self.sent.is_empty() {
            self.discontinuous = false;
        }
        Some(micros)
    }

    fn position(&self, sequence: u32) -> Option<usize> {
        if self.discontinuous {
            return self
                .sent
                .iter()
                .position(|(candidate, _, _)| *candidate == sequence);
        }
        // Contiguous sequences map directly from the deque's front, including
        // u32 wrap. Sparse/duplicate/out-of-order arrivals use the old search.
        let offset = sequence.wrapping_sub(self.sent.front()?.0) as usize;
        (offset < self.sent.len()).then_some(offset)
    }

    fn take_report(&mut self) -> Report {
        let samples = std::mem::replace(&mut self.samples, std::mem::take(&mut self.spare_samples));
        let decode_samples = std::mem::replace(
            &mut self.decode_samples,
            std::mem::take(&mut self.spare_decode_samples),
        );
        Report {
            samples,
            decode_samples,
            lost: std::mem::take(&mut self.lost),
            inflight: self.sent.len(),
        }
    }
}

fn keep_capacity(spare: &mut Vec<u32>, mut returned: Vec<u32>) {
    if returned.capacity() > spare.capacity() {
        std::mem::swap(spare, &mut returned);
    }
}

struct Report {
    samples: Vec<u32>,
    decode_samples: Vec<u32>,
    lost: u64,
    inflight: usize,
}
impl Report {
    fn log(&mut self) {
        if self.samples.is_empty() {
            if self.lost > 0 {
                info!(
                    "Latency: no frames acknowledged by the tablet ({} aged out)",
                    self.lost
                );
            }
            return;
        }
        self.samples.sort_unstable();
        info!(
            "Latency packet-ready→render-ACK (host clock): p50 {:.1}ms  p95 {:.1}ms  max {:.1}ms  ({} samples, {} in flight, {} aged out)",
            percentile(&self.samples, 0.50), percentile(&self.samples, 0.95), percentile(&self.samples, 1.0),
            self.samples.len(), self.inflight, self.lost
        );
        self.log_decode();
    }
    fn log_decode(&mut self) {
        if self.decode_samples.is_empty() {
            return;
        }
        self.decode_samples.sort_unstable();
        info!(
            "Latency tablet arrival→render-callback (tablet clock): p50 {:.1}ms  p95 {:.1}ms  ({} samples)",
            percentile(&self.decode_samples, 0.50),
            percentile(&self.decode_samples, 0.95),
            self.decode_samples.len()
        );
    }
}
fn percentile(values: &[u32], p: f64) -> f64 {
    let index = ((values.len() as f64 - 1.0) * p).round() as usize;
    values[index] as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn t457_reports_separate_clock_intervals_without_subtracting_percentiles() {
        let buffer = LogBuffer(Arc::new(Mutex::new(Vec::new())));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let mut report = Report {
            samples: vec![90_000, 10_000, 20_000],
            decode_samples: vec![30_000, 5_000],
            lost: 2,
            inflight: 1,
        };
        tracing::subscriber::with_default(subscriber, || report.log());
        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("Latency packet-ready→render-ACK (host clock): p50 20.0ms  p95 90.0ms  max 90.0ms  (3 samples, 1 in flight, 2 aged out)"), "T457: {output}");
        assert!(output.contains("Latency tablet arrival→render-callback (tablet clock): p50 30.0ms  p95 30.0ms  (2 samples)"), "T457: {output}");
        assert!(!output.contains("wire"), "T457: no inferred network metric");
        assert!(
            !output.contains("encode→display"),
            "T457: exclude unmeasured stages"
        );
    }

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

    #[test]
    fn t404_report_keeps_sample_storage_for_reuse() {
        let tracker = LatencyTracker::new();
        for sequence in 0..32 {
            tracker.on_encoded(sequence);
            tracker.on_rendered(sequence, 10);
        }
        let capacity = {
            let mut state = tracker.inner.lock().unwrap();
            state.last_report = Some(Instant::now() - std::time::Duration::from_secs(6));
            state.samples.capacity() + state.decode_samples.capacity()
        };
        tracker.maybe_report();
        let state = tracker.inner.lock().unwrap();
        assert!(state.samples.is_empty() && state.decode_samples.is_empty());
        assert!(
            state.spare_samples.capacity() + state.spare_decode_samples.capacity() >= capacity,
            "T404: report discarded reusable sample backing"
        );
    }

    #[test]
    fn t404_sparse_duplicate_and_wrapped_sequences_keep_ack_eviction_order() {
        for sequences in [vec![5, 7, 7, 8], vec![8, 7, 9, 7], vec![u32::MAX, 0, 1, 2]] {
            let tracker = LatencyTracker::new();
            for &sequence in &sequences {
                tracker.on_encoded(sequence);
            }
            tracker.on_rendered(sequences[1], 10);
            let state = tracker.inner.lock().unwrap();
            assert_eq!(
                state
                    .sent
                    .iter()
                    .map(|&(sequence, _, _)| sequence)
                    .collect::<Vec<_>>(),
                sequences[2..]
            );
            assert_eq!(state.samples.len(), 1);
            drop(state);
            tracker.on_rendered(1_234_567, 10);
            let state = tracker.inner.lock().unwrap();
            assert_eq!(state.samples.len(), 1);
            assert_eq!(state.lost, 0);
        }
    }

    fn t404_acknowledge_range(tracker: &LatencyTracker, first: u32, count: u32) {
        for sequence in first..first + count {
            tracker.on_encoded(sequence);
            tracker.on_rendered(sequence, i64::from(sequence + 1));
        }
    }
    #[test]
    fn t404_report_recycling_preserves_new_and_overlapping_samples() {
        let tracker = LatencyTracker::new();
        t404_acknowledge_range(&tracker, 0, 16);
        let first = tracker.inner.lock().unwrap().take_report();
        t404_acknowledge_range(&tracker, 16, 16);
        let second = tracker.inner.lock().unwrap().take_report();
        t404_acknowledge_range(&tracker, 32, 16);
        assert_eq!(first.decode_samples, (1..=16).collect::<Vec<_>>());
        assert_eq!(second.decode_samples, (17..=32).collect::<Vec<_>>());
        tracker.recycle(first);
        tracker.recycle(second);
        let state = tracker.inner.lock().unwrap();
        assert_eq!(state.decode_samples, (33..=48).collect::<Vec<_>>());
        assert_eq!(state.samples.len(), 16);
        assert!(state.spare_samples.is_empty());
        assert!(state.spare_samples.capacity() >= 16);
        assert!(state.spare_decode_samples.capacity() >= 16);
    }
    #[test]
    fn t404_capacity_loss_and_duplicate_acknowledgements_keep_prior_meaning() {
        let tracker = LatencyTracker::new();
        for index in 0..260 {
            tracker.on_encoded(u32::MAX.wrapping_sub(100).wrapping_add(index));
        }
        let last = tracker.inner.lock().unwrap().sent.back().unwrap().0;
        tracker.on_rendered(last, i64::MAX);
        tracker.on_rendered(last, 10);
        let state = tracker.inner.lock().unwrap();
        assert_eq!(state.lost, 4);
        assert!(state.sent.is_empty());
        assert_eq!(state.samples.len(), 1);
        assert_eq!(state.decode_samples, [u32::MAX]);
        assert!(!state.discontinuous);
        assert_eq!(percentile(&[1, 7, 9, 17], 0.5), 0.009);
        assert_eq!(percentile(&[1, 7, 9, 17], 0.95), 0.017);
    }
}
