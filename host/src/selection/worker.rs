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
    let candidates = calibrate(base, snapshot).await;
    supervise(settings, snapshot, latency, candidates).await;
}

async fn supervise(
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    latency: &LatencyTracker,
    candidates: Vec<Candidate>,
) {
    let key = Key::new(snapshot);
    let mut pending = candidates;
    let mut fallback = fallback_encoder(snapshot).to_string();
    while let Some((name, remaining)) = choose(settings, &key, &fallback, pending, |name| {
        let key = &key;
        let decoder = settings.borrow().decoder_choice().cloned();
        async move { rendered(latency, key, &name, decoder.as_ref()).await }
    })
    .await
    {
        let decoder = settings.borrow().decoder_choice().cloned();
        super::health::failed(latency, &key, &name, decoder.as_ref()).await;
        tracing::warn!(encoder = %name, "Verified encoder lost render progress; trying remaining compatible candidates");
        // Never return to a candidate that failed in this settings/peer epoch.
        pending = remaining;
        fallback = "libx264".into();
        publish(
            settings,
            &key,
            &fallback,
            "Render progress lost; using H.264 during recovery backoff",
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn choose<F: Future<Output = bool>>(
    settings: &watch::Sender<EncoderSettings>,
    key: &Key,
    fallback: &str,
    candidates: Vec<Candidate>,
    mut verify: impl FnMut(String) -> F,
) -> Option<(String, Vec<Candidate>)> {
    let mut candidates = candidates.into_iter();
    while let Some(candidate) = candidates.next() {
        let reason = format!("Host probe: {:.1} FPS, packet-interval p95 {:.2} ms; {} decoder advertised; awaiting render ACKs",
            candidate.measurement.fps, candidate.measurement.p95_us as f64 / 1000.0,
            if candidate.hardware { "hardware" } else { "software/unknown" });
        if !publish_choice(
            settings,
            key,
            &candidate.measurement.encoder,
            &reason,
            candidate.decoder.clone(),
        ) {
            return None;
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
                    decoder: candidate.decoder.clone(),
                });
                true
            });
            return Some((candidate.measurement.encoder, candidates.collect()));
        }
        tracing::warn!(encoder = %candidate.measurement.encoder, "Automatic codec trial produced no verified render ACKs; trying next compatible candidate");
    }
    publish(
        settings,
        key,
        fallback,
        "Preserved fallback: compatible encoder trials failed or did not render",
    );
    None
}

fn publish(
    settings: &watch::Sender<EncoderSettings>,
    key: &Key,
    encoder: &str,
    reason: &str,
) -> bool {
    publish_choice(settings, key, encoder, reason, None)
}

fn publish_choice(
    settings: &watch::Sender<EncoderSettings>,
    key: &Key,
    encoder: &str,
    reason: &str,
    decoder: Option<uscreen_config::negotiation::DecoderChoice>,
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
            decoder: decoder.clone(),
        });
        tracing::info!(encoder, reason, "Automatic encoder selection");
        true
    })
}

async fn rendered(
    latency: &LatencyTracker,
    key: &Key,
    name: &str,
    decoder: Option<&uscreen_config::negotiation::DecoderChoice>,
) -> bool {
    let mut updates = latency.activity_updates();
    let previous = latency.encoder_evidence().map(|e| (e.epoch, e.rendered()));
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if matches_evidence(latency.encoder_evidence(), key, name, previous, decoder) {
                return true;
            }
            if updates.changed().await.is_err() {
                return false;
            }
        }
    })
    .await
    .unwrap_or(false)
}
fn matches_evidence(
    evidence: Option<Arc<crate::latency::EncoderEvidence>>,
    key: &Key,
    name: &str,
    previous: Option<(u64, u64)>,
    decoder: Option<&uscreen_config::negotiation::DecoderChoice>,
) -> bool {
    evidence.is_some_and(|e| {
        let before = previous
            .filter(|&(epoch, _)| epoch == e.epoch)
            .map(|(_, count)| count)
            .unwrap_or(0);
        e.active()
            && e.name == name
            && e.format == key.format
            && e.decoder.as_ref() == decoder
            && e.rendered().saturating_sub(before) >= 3
    })
}

#[derive(Debug)]
struct Candidate {
    measurement: Measurement,
    hardware: bool,
    decoder: Option<uscreen_config::negotiation::DecoderChoice>,
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
            Ok(measurement) => {
                if let Some(candidate) = compatible_candidate(snapshot, measurement, base.ten_bit) {
                    candidates.push(candidate);
                }
            }
            Err(error) => {
                tracing::info!(encoder = encoder.name, %error, "Automatic encoder probe rejected candidate")
            }
        }
    }
    candidates.sort_by(|a, b| rank(a, b, snapshot.fps));
    candidates
}

fn compatible_candidate(
    settings: &EncoderSettings,
    measurement: Measurement,
    ten_bit: bool,
) -> Option<Candidate> {
    let caps = settings.decoders.as_ref()?;
    let codec = crate::media::Codec::from_encoder(&measurement.encoder);
    let decoder = if caps.protocol == 2 {
        if !supported_output(&measurement, codec, ten_bit) {
            return None;
        }
        Some(caps.choose(measurement.stream.as_ref()?, true)?)
    } else {
        None
    };
    let hardware = match &decoder {
        Some(choice) => caps.details.iter().any(|d| {
            d.name == choice.name && d.codec == codec.wire_name() && d.hardware == Some(true)
        }),
        None => caps.hardware.iter().any(|name| name == codec.wire_name()),
    };
    Some(Candidate {
        measurement,
        hardware,
        decoder,
    })
}

fn supported_output(measurement: &Measurement, codec: crate::media::Codec, ten_bit: bool) -> bool {
    let depth = if ten_bit && codec == crate::media::Codec::Hevc {
        10
    } else {
        8
    };
    measurement
        .stream
        .as_ref()
        .is_some_and(|stream| stream.codec == codec.wire_name() && stream.format.depth == depth)
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
