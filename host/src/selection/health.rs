//! Encoder-epoch render progress. Idle content alone never triggers recovery.
use super::Key;
use crate::latency::{EncoderEvidence, LatencyTracker};
use std::time::Duration;
use tokio::time::Instant;

const STALL_WINDOW: Duration = Duration::from_secs(6);

#[derive(Default)]
struct Progress {
    epoch: Option<u64>,
    rendered: u64,
    pending_since: Option<Instant>,
    recovering: bool,
}
impl Progress {
    fn deadline(&mut self, evidence: &EncoderEvidence, now: Instant) -> Option<Instant> {
        self.observe_epoch(evidence);
        if evidence.rendered() > self.rendered {
            self.rendered = evidence.rendered();
            self.pending_since = None;
            self.recovering = false;
        }
        let outstanding = evidence.unrendered_output();
        if outstanding > 0 || !evidence.active() {
            self.pending_since.get_or_insert(now);
        }
        if outstanding >= 3 || !evidence.active() || self.recovering {
            return self.pending_since.map(|at| at + STALL_WINDOW);
        }
        None
    }
    fn observe_epoch(&mut self, evidence: &EncoderEvidence) {
        if self.epoch == Some(evidence.epoch) {
            return;
        }
        self.recovering = self.pending_since.is_some();
        self.epoch = Some(evidence.epoch);
        self.rendered = 0;
    }
}

pub(super) async fn failed(
    latency: &LatencyTracker,
    key: &Key,
    name: &str,
    decoder: Option<&uscreen_config::negotiation::DecoderChoice>,
) {
    let mut updates = latency.activity_updates();
    let mut progress = Progress::default();
    loop {
        // Read after subscribing so activity between inspection and suspension
        // remains visible. Retired encoders' late ACKs cannot refresh this timer.
        let deadline = latency
            .encoder_evidence()
            .filter(|e| e.name == name && e.format == key.format && e.decoder.as_ref() == decoder)
            .and_then(|e| progress.deadline(&e, Instant::now()));
        if deadline.is_some_and(|at| Instant::now() >= at) {
            return;
        }
        tokio::select! {
            _ = updates.changed() => {},
            _ = wait_deadline(deadline) => {},
        }
    }
}
async fn wait_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
