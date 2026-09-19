//! Compare a bounded subset in the current session with measurement provenance.
use super::*;
use crate::selection::trial::Observation;

fn shortlist(mut candidates: Vec<Candidate>, fps: u32) -> Vec<Candidate> {
    let Some(reference) = candidates
        .iter()
        .find(|c| c.measurement.encoder == "libx264")
        .and_then(|c| c.measurement.quality_db)
        .filter(|q| q.is_finite())
    else {
        return Vec::new();
    };
    candidates.retain(|c| quality_capacity(c, reference, fps));
    // Keep the known fallback and profile comparison ahead of exploratory codecs.
    candidates.sort_by_key(|c| match c.measurement.encoder.as_str() {
        "libx264" => 0,
        "h264_vaapi_baseline" => 1,
        _ => 2,
    });
    candidates.truncate(4);
    candidates
}

pub(super) fn quality_capacity(candidate: &Candidate, reference: f64, fps: u32) -> bool {
    candidate.measurement.fps >= f64::from(fps)
        && candidate
            .measurement
            .quality_db
            .is_some_and(|q| q.is_finite() && q >= reference - 0.5)
}

fn order(candidates: &mut Vec<Candidate>) {
    if candidates.is_empty() {
        return;
    }
    let best = (1..candidates.len()).fold(0, |best, index| {
        if prefer(&candidates[index], &candidates[best]) {
            index
        } else {
            best
        }
    });
    let winner = candidates.remove(best);
    candidates.sort_by_key(|c| c.observation.as_ref().unwrap().p95_us);
    candidates.insert(0, winner);
}

fn prefer(candidate: &Candidate, previous: &Candidate) -> bool {
    let current = candidate.observation.as_ref().unwrap();
    let prior = previous.observation.as_ref().unwrap();
    let quality_gain = candidate.measurement.quality_db.unwrap_or(0.0)
        - previous.measurement.quality_db.unwrap_or(0.0);
    current.improves(prior) || (current.similar_speed(prior) && quality_gain >= 1.0)
}

pub(super) async fn benchmark(
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    latency: &LatencyTracker,
    candidates: Vec<Candidate>,
) -> Vec<Candidate> {
    let key = Key::new(snapshot);
    benchmark_with(settings, snapshot, candidates, |candidate| {
        let key = &key;
        async move {
            super::super::trial::observe(
                latency,
                key,
                &candidate.measurement.encoder,
                candidate.decoder.as_ref(),
            )
            .await
        }
    })
    .await
}

async fn benchmark_with<F: Future<Output = Option<Observation>>>(
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    candidates: Vec<Candidate>,
    mut measure: impl FnMut(Candidate) -> F,
) -> Vec<Candidate> {
    let key = Key::new(snapshot);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(36);
    let mut measured = Vec::new();
    for candidate in shortlist(candidates, snapshot.fps) {
        record_trial(
            settings,
            &key,
            candidate,
            deadline,
            &mut measure,
            &mut measured,
        )
        .await;
    }
    order(&mut measured);
    let alternatives = measured
        .first()
        .map(|best| variants(snapshot, best))
        .unwrap_or_default();
    for candidate in alternatives {
        record_trial(
            settings,
            &key,
            candidate,
            deadline,
            &mut measure,
            &mut measured,
        )
        .await;
    }
    order(&mut measured);
    measured
}

async fn record_trial<F: Future<Output = Option<Observation>>>(
    settings: &watch::Sender<EncoderSettings>,
    key: &Key,
    mut candidate: Candidate,
    deadline: tokio::time::Instant,
    measure: &mut impl FnMut(Candidate) -> F,
    measured: &mut Vec<Candidate>,
) {
    if tokio::time::Instant::now() >= deadline {
        return;
    }
    if !publish_choice(
        settings,
        key,
        &candidate.measurement.encoder,
        "Measuring packet-ready to render-ACK timing for this session",
        candidate.decoder.clone(),
    ) {
        return;
    }
    candidate.observation = tokio::time::timeout_at(deadline, measure(candidate.clone()))
        .await
        .ok()
        .flatten();
    if let Some(observation) = &candidate.observation {
        tracing::info!(encoder = %candidate.measurement.encoder, decoder = ?candidate.decoder,
            observation = %serde_json::to_string(observation).unwrap(), "Automatic profile observation");
        measured.push(candidate);
    }
}

fn variants(snapshot: &EncoderSettings, best: &Candidate) -> Vec<Candidate> {
    let Some(selected) = &best.decoder else {
        return Vec::new();
    };
    let mut variants = Vec::new();
    let mut unhinted = selected.clone();
    unhinted.low_latency = false;
    unhinted.operating_rate = None;
    if &unhinted != selected {
        variants.push(variant(snapshot, best, unhinted));
    }
    let alternative = snapshot
        .decoders
        .as_ref()
        .unwrap()
        .choices(&selected.stream, true)
        .into_iter()
        .find(|choice| choice.name != selected.name);
    if let Some(choice) = alternative {
        variants.push(variant(snapshot, best, choice));
    }
    variants
}

fn variant(
    snapshot: &EncoderSettings,
    best: &Candidate,
    choice: uscreen_config::negotiation::DecoderChoice,
) -> Candidate {
    let hardware = snapshot.decoders.as_ref().unwrap().details.iter().any(|d| {
        d.name == choice.name && d.codec == choice.stream.codec && d.hardware == Some(true)
    });
    Candidate {
        decoder: Some(choice),
        observation: None,
        hardware,
        ..best.clone()
    }
}

pub(super) fn reason(candidate: &Candidate) -> Option<String> {
    let result = candidate.observation.as_ref()?;
    let scope = if candidate.cached {
        "Historical"
    } else {
        "This-session"
    };
    let probe_scope = if candidate.cached {
        "current host probe"
    } else {
        "first-frame probe"
    };
    Some(format!("{scope} packet-ready→render-ACK p50/p95/p99 {:.1}/{:.1}/{:.1} ms; {:.1} ACK FPS, {} samples, {:.1}% delivery; {probe_scope} PSNR {:.2} dB; awaiting render ACKs",
        result.p50_us as f64 / 1000.0, result.p95_us as f64 / 1000.0, result.p99_us as f64 / 1000.0,
        result.ack_fps, result.samples, result.delivery_permille as f64 / 10.0,
        candidate.measurement.quality_db.unwrap_or(0.0)))
}

#[cfg(test)]
mod tests {
    use super::super::tests::candidate;
    use super::*;

    fn measured(name: &str, latency: u32) -> Candidate {
        let mut row = candidate(name, true, 120.0, 1);
        row.observation = Some(Observation {
            p50_us: latency,
            p95_us: latency,
            p99_us: latency,
            ack_fps: 60.0,
            startup_us: 50_000,
            delivery_permille: 1000,
            samples: 120,
        });
        row
    }

    #[tokio::test(start_paused = true)]
    async fn t479_benchmark_compares_later_trials_and_retains_success_after_deadline() {
        let snapshot = super::super::tests::settings();
        let (tx, _rx) = watch::channel(snapshot.clone());
        let mut slow = measured("libx264", 30_000);
        let fast = measured("h264_vaapi_baseline", 12_000);
        let rows = benchmark_with(&tx, &snapshot, vec![slow.clone(), fast], |c| {
            std::future::ready(c.observation)
        })
        .await;
        assert_eq!(rows[0].measurement.encoder, "h264_vaapi_baseline");
        slow.measurement.encoder = "timed-out".into();
        let rows = benchmark_with(
            &tx,
            &snapshot,
            vec![measured("libx264", 30_000), slow],
            |c| async move {
                if c.measurement.encoder == "timed-out" {
                    std::future::pending::<()>().await;
                }
                c.observation
            },
        )
        .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].measurement.encoder, "libx264");
    }

    #[test]
    fn t479_render_latency_beats_host_probe_order_without_switching_for_noise() {
        let mut rows = vec![
            measured("h264_vaapi", 30_000),
            measured("h264_vaapi_baseline", 12_000),
        ];
        order(&mut rows);
        assert_eq!(rows[0].measurement.encoder, "h264_vaapi_baseline");
        rows.push(measured("libx264", 11_000));
        order(&mut rows);
        assert_eq!(rows[0].measurement.encoder, "h264_vaapi_baseline");
    }

    #[test]
    fn t479_latency_tie_prefers_materially_better_measured_fidelity() {
        let mut software = measured("libx264", 12_000);
        software.measurement.quality_db = Some(35.0);
        let mut hardware = measured("h264_vaapi_baseline", 13_000);
        hardware.measurement.quality_db = Some(59.0);
        let mut slow = measured("slow", 25_000);
        slow.measurement.quality_db = Some(70.0);
        let mut rows = vec![software, hardware, slow];
        order(&mut rows);
        assert_eq!(rows[0].measurement.encoder, "h264_vaapi_baseline");
    }

    #[test]
    fn t479_trials_bound_work_and_require_measured_quality_and_capacity() {
        let good = candidate("libx264", true, 120.0, 5);
        let slow = candidate("slow", true, 20.0, 2);
        let mut poor = candidate("poor", true, 120.0, 1);
        poor.measurement.quality_db = Some(35.0);
        let mut unknown = candidate("unknown", true, 120.0, 1);
        unknown.measurement.quality_db = None;
        let rows = shortlist(vec![poor, unknown, slow, good.clone()], 60);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].measurement.encoder, "libx264");
        let many = shortlist(vec![good; 20], 60);
        assert!(many.len() <= 4);
        assert!(shortlist(vec![candidate("no-reference", true, 120.0, 1)], 60).is_empty());
    }
}
