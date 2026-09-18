use super::{Key, Selected};
use crate::{
    capture::{
        probe::{self, Measurement},
        CaptureConfig,
    },
    latency::LatencyTracker,
    media::EncoderSettings,
};
use std::{cmp::Ordering, future::Future, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle};

// Calibrate one encoder at a time across all tablet sessions. Current streams
// continue normally; concurrent probes would distort comparative measurements.
static ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

pub(crate) fn spawn(
    config: CaptureConfig,
    settings: watch::Sender<EncoderSettings>,
    display: watch::Receiver<bool>,
    stop: watch::Receiver<bool>,
    latency: LatencyTracker,
) -> JoinHandle<()> {
    tokio::spawn(run(config, settings, display, stop, latency))
}

async fn run(
    config: CaptureConfig,
    settings: watch::Sender<EncoderSettings>,
    mut display: watch::Receiver<bool>,
    mut stop: watch::Receiver<bool>,
    latency: LatencyTracker,
) {
    let mut updates = settings.subscribe();
    let mut completed = None;
    loop {
        if stopped(&stop) || display.has_changed().is_err() {
            return;
        }
        let snapshot = updates.borrow_and_update().clone();
        let key = Key::new(&snapshot);
        if eligible(&snapshot, *display.borrow()) && completed.as_ref() != Some(&key) {
            let finished = until_changed(
                &key,
                &mut updates,
                &mut display,
                &mut stop,
                optimize(&config, &settings, &snapshot, &latency),
            )
            .await;
            if finished.is_some() {
                completed = Some(key);
            } else {
                publish(
                    &settings,
                    &key,
                    fallback_encoder(&snapshot),
                    "Cancelled trial; restored fallback",
                );
            }
            continue;
        }
        tokio::select! {
            r = updates.changed() => if r.is_err() { return; },
            r = display.changed() => if r.is_err() { return; },
            _ = stop.changed() => return,
        }
    }
}

fn stopped(stop: &watch::Receiver<bool>) -> bool {
    *stop.borrow() || stop.has_changed().is_err()
}
fn eligible(settings: &EncoderSettings, visible: bool) -> bool {
    visible
        && settings.encoder == "auto"
        && settings.geometry_ready
        && settings.decoder_supports(crate::media::Codec::H264)
}

async fn until_changed<T>(
    key: &Key,
    updates: &mut watch::Receiver<EncoderSettings>,
    display: &mut watch::Receiver<bool>,
    stop: &mut watch::Receiver<bool>,
    work: impl Future<Output = T>,
) -> Option<T> {
    tokio::pin!(work);
    loop {
        if stopped(stop) || !*display.borrow() || !key.matches(&updates.borrow()) {
            return None;
        }
        tokio::select! {
            result = &mut work => return Some(result),
            r = updates.changed() => if r.is_err() { return None; },
            r = display.changed() => if r.is_err() { return None; },
            _ = stop.changed() => return None,
        }
    }
}

async fn optimize(
    base: &CaptureConfig,
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    latency: &LatencyTracker,
) {
    let key = Key::new(snapshot);
    let fallback = fallback_encoder(snapshot).to_string();
    let candidates = calibrate(base, snapshot).await;
    choose(settings, &key, &fallback, candidates, |name| {
        let key = &key;
        async move { rendered(latency, key, &name).await }
    })
    .await;
}

async fn choose<F: Future<Output = bool>>(
    settings: &watch::Sender<EncoderSettings>,
    key: &Key,
    fallback: &str,
    candidates: Vec<Candidate>,
    mut verify: impl FnMut(String) -> F,
) {
    for candidate in candidates {
        let reason = format!("Host probe: {:.1} FPS, packet-interval p95 {:.2} ms; {} decoder advertised; awaiting render ACKs",
            candidate.measurement.fps, candidate.measurement.p95_us as f64 / 1000.0,
            if candidate.hardware { "hardware" } else { "software/unknown" });
        if !publish(settings, key, &candidate.measurement.encoder, &reason) {
            return;
        }
        if verify(candidate.measurement.encoder.clone()).await {
            let verified = reason.replace(
                "awaiting render ACKs",
                "render ACKs verified; decoder speed not benchmarked",
            );
            settings.send_if_modified(|current| {
                if !key.matches(current) {
                    return false;
                }
                current.selection = Some(Selected {
                    key: key.clone(),
                    encoder: candidate.measurement.encoder.clone(),
                    reason: verified.clone(),
                    verified: true,
                });
                true
            });
            return;
        }
        tracing::warn!(encoder = %candidate.measurement.encoder, "Automatic codec trial produced no verified render ACKs; trying next compatible candidate");
    }
    publish(
        settings,
        key,
        fallback,
        "Preserved fallback: compatible encoder trials failed or did not render",
    );
}

fn publish(
    settings: &watch::Sender<EncoderSettings>,
    key: &Key,
    encoder: &str,
    reason: &str,
) -> bool {
    settings.send_if_modified(|current| {
        if !key.matches(current) {
            return false;
        }
        current.selection = Some(Selected {
            key: key.clone(),
            encoder: encoder.into(),
            reason: reason.into(),
            verified: false,
        });
        tracing::info!(encoder, reason, "Automatic encoder selection");
        true
    })
}

async fn rendered(latency: &LatencyTracker, key: &Key, name: &str) -> bool {
    let previous = latency.encoder_evidence().map(|e| (e.epoch, e.rendered()));
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if matches_evidence(latency.encoder_evidence(), key, name, previous) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_ok()
}
fn matches_evidence(
    evidence: Option<Arc<crate::latency::EncoderEvidence>>,
    key: &Key,
    name: &str,
    previous: Option<(u64, u64)>,
) -> bool {
    evidence.is_some_and(|e| {
        let before = previous
            .filter(|&(epoch, _)| epoch == e.epoch)
            .map(|(_, count)| count)
            .unwrap_or(0);
        e.name == name && e.format == key.format && e.rendered().saturating_sub(before) >= 3
    })
}

#[derive(Debug)]
struct Candidate {
    measurement: Measurement,
    hardware: bool,
}

async fn calibrate(base: &CaptureConfig, snapshot: &EncoderSettings) -> Vec<Candidate> {
    let _permit = ADMISSION.acquire().await.unwrap();
    let mut candidates = Vec::new();
    for encoder in uscreen_config::encoding::ENCODERS {
        let codec = crate::media::Codec::from_encoder(encoder.name);
        if !snapshot.decoder_supports(codec) {
            continue;
        }
        let config = probe_config(base, snapshot, encoder.name);
        match probe::measure(&config).await {
            Ok(measurement) => candidates.push(Candidate {
                measurement,
                hardware: snapshot
                    .decoders
                    .as_ref()
                    .unwrap()
                    .hardware
                    .iter()
                    .any(|name| name == codec.wire_name()),
            }),
            Err(error) => {
                tracing::info!(encoder = encoder.name, %error, "Automatic encoder probe rejected candidate")
            }
        }
    }
    candidates.sort_by(|a, b| rank(a, b, snapshot.fps));
    candidates
}

fn probe_config(base: &CaptureConfig, settings: &EncoderSettings, encoder: &str) -> CaptureConfig {
    let (width, height) = settings.video_dimensions();
    CaptureConfig {
        encoder: encoder.into(),
        width,
        height,
        stream_scale: 1,
        fps: settings.fps,
        bitrate: settings.bitrate,
        quality: settings.quality,
        ..base.clone()
    }
}

fn rank(a: &Candidate, b: &Candidate, fps: u32) -> Ordering {
    let score = |candidate: &Candidate| {
        (
            candidate.measurement.fps < f64::from(fps),
            !candidate.hardware,
            candidate.measurement.p95_us,
            candidate.measurement.first_us,
        )
    };
    score(a)
        .cmp(&score(b))
        .then_with(|| a.measurement.encoder.cmp(&b.measurement.encoder))
}

#[cfg(test)]
mod tests;

fn fallback_encoder(settings: &EncoderSettings) -> &str {
    settings
        .selection
        .as_ref()
        .filter(|s| s.verified && s.key.matches(settings))
        .map(|s| s.encoder.as_str())
        .unwrap_or("libx264")
}
