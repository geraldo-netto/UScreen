//! Compare a bounded subset in the current session with measurement provenance.
use super::*;
use crate::selection::trial::Observation;

fn shortlist(mut candidates: Vec<Candidate>, fps: u32) -> Vec<Candidate> {
    let Some(reference) = quality_reference(&candidates)
    else {
        return Vec::new();
    };
    candidates.retain(|c| quality_capacity(c, reference, fps));
    // Keep the known fallback and profile comparison ahead of exploratory codecs.
    candidates.sort_by_key(|c| (match c.measurement.encoder.as_str() {
        "libx264" => 0,
        "h264_vaapi_baseline" => 1,
        _ => 2,
    }, c.measurement.workers_requested));
    candidates.truncate(4);
    candidates
}

pub(super) fn quality_reference(candidates: &[Candidate]) -> Option<f64> {
    candidates.iter().filter(|c| c.measurement.encoder == "libx264")
        .min_by_key(|c| c.measurement.workers_requested)
        .and_then(|c| c.measurement.quality_db).filter(|q| q.is_finite())
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
    if !resources_acceptable(current, prior) {
        return false;
    }
    let quality_gain = candidate.measurement.quality_db.unwrap_or(0.0)
        - previous.measurement.quality_db.unwrap_or(0.0);
    current.improves(prior) || (current.similar_speed(prior) && quality_gain >= 1.0)
}

fn resources_acceptable(current: &Observation, previous: &Observation) -> bool {
    current.resources.as_ref().zip(previous.resources.as_ref())
        .is_some_and(|(current, previous)| current.acceptable(previous))
}

pub(super) async fn benchmark(
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    latency: &LatencyTracker,
    candidates: Vec<Candidate>,
) -> Vec<Candidate> {
    let key = Key::new(snapshot);
    if !super::super::trial::capture_ready(latency, &key).await {
        tracing::info!("Automatic trials skipped: capture did not become ready within 15s");
        return Vec::new();
    }
    benchmark_with(settings, snapshot, candidates, |candidate| {
        let key = &key;
        async move {
            super::super::trial::observe(
                latency,
                key,
                &candidate.measurement.encoder,
                candidate.decoder.as_ref(),
                candidate.measurement.workers_requested,
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
    let require_one = candidates.iter().any(|c| c.measurement.workers_requested == 1);
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
    retain_valid_budgets(&mut measured, require_one);
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

fn retain_valid_budgets(measured: &mut Vec<Candidate>, require_one: bool) {
    if require_one && !measured.iter().any(|c| c.measurement.workers_requested == 1) {
        measured.retain(|c| c.measurement.workers_requested <= 1);
    }
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
        candidate.measurement.workers_requested,
    ) {
        return;
    }
    candidate.observation = tokio::time::timeout_at(deadline, measure(candidate.clone()))
        .await
        .ok()
        .flatten();
    if let Some(observation) = &candidate.observation {
        tracing::info!(encoder = %candidate.measurement.encoder, decoder = ?candidate.decoder,
            workers = candidate.measurement.workers_requested, effective_workers = ?candidate.measurement.workers_effective,
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
    choice: blent_config::negotiation::DecoderChoice,
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
        candidate.measurement.quality_db.unwrap_or(0.0)) + &format!("; workers requested {}, effective {}; encoder resources {:?}",
            candidate.measurement.workers_requested,
            candidate.measurement.workers_effective.map_or_else(|| "unknown".into(), |n| n.to_string()), result.resources))
}

#[cfg(test)]
mod tests {
    use super::super::tests::candidate;
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn t627_live_trials_wait_for_matching_capture_output_before_spending_window() {
        let snapshot = super::super::tests::settings();
        let (tx, _rx) = watch::channel(snapshot.clone());
        let tracker = LatencyTracker::new();
        let work = tokio::spawn({
            let tx = tx.clone();
            let tracker = tracker.clone();
            let snapshot = snapshot.clone();
            async move { benchmark(&tx, &snapshot, &tracker, vec![candidate("libx264", true, 120.0, 1)]).await }
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(7)).await;
        assert!(tx.borrow().selection.is_none(), "T627: trial budget started before capture was ready");
        assert!(!work.is_finished());
        let evidence = tracker.encoder_started("libx264", Key::new(&snapshot).format);
        tokio::task::yield_now().await;
        assert!(tx.borrow().selection.is_none(), "spawning an encoder is not captured output");
        tracker.on_encoded_for(tracker.next_sequence(), &evidence);
        tokio::task::yield_now().await;
        assert!(tx.borrow().selection.is_some());
        for _ in 0..18 {
            let seq = tracker.next_sequence();
            tracker.on_encoded_for(seq, &evidence);
            tracker.on_rendered(seq, 100);
            tokio::time::advance(Duration::from_millis(200)).await;
        }
        assert_eq!(work.await.unwrap().len(), 1);
    }

    #[test]
    fn t612_worker_trials_start_with_one_and_require_resource_evidence() {
        use crate::selection::resources::Usage;
        let mut one = measured("libx264", 30_000);
        one.measurement.workers_requested = 1;
        let mut two = measured("libx264", 10_000);
        two.measurement.workers_requested = 2;
        let rows = shortlist(vec![two.clone(), one.clone()], 60);
        assert_eq!(rows[0].measurement.workers_requested, 1);
        two.observation.as_mut().unwrap().resources = None;
        assert!(!prefer(&two, &one));
        one.observation.as_mut().unwrap().resources = Some(Usage { cpu_us_per_frame: 1000.0, peak_rss_bytes: 100_000 });
        two.observation.as_mut().unwrap().resources = Some(Usage { cpu_us_per_frame: 1050.0, peak_rss_bytes: 110_000 });
        assert!(prefer(&two, &one));
        two.observation.as_mut().unwrap().resources.as_mut().unwrap().peak_rss_bytes = 121_000;
        assert!(!prefer(&two, &one));
        two.measurement.quality_db = Some(1.0);
        assert_eq!(shortlist(vec![one, two], 60).len(), 1);
    }

    #[test]
    fn t612_worker_quality_uses_one_worker_even_when_probe_rank_differs() {
        let mut one = measured("libx264", 30_000);
        one.measurement.workers_requested = 1;
        one.measurement.quality_db = Some(40.0);
        let mut two = measured("libx264", 10_000);
        two.measurement.workers_requested = 2;
        two.measurement.quality_db = Some(38.0);
        let rows = shortlist(vec![two, one], 60);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].measurement.workers_requested, 1);
    }

    #[test]
    fn t612_failed_one_worker_baseline_cannot_certify_higher_auto_budget() {
        let mut two = measured("libx264", 10_000);
        two.measurement.workers_requested = 2;
        let mut rows = vec![two.clone()];
        retain_valid_budgets(&mut rows, true);
        assert!(rows.is_empty());
        rows.push(two);
        retain_valid_budgets(&mut rows, false);
        assert_eq!(rows.len(), 1, "manual budget needs no automatic baseline");
    }

    #[tokio::test(start_paused = true)]
    async fn t497_hint_and_decoder_variants_require_new_measurements() {
        let mut snapshot = super::super::tests::settings();
        let mut report: crate::media::DecoderCapabilities = serde_json::from_str(include_str!(
            "../../../../testdata/decoder-capabilities-v2.json"
        ))
        .unwrap();
        report.scope = Some(snapshot.decoder_epoch.to_string());
        let mut software = report.details[0].clone();
        software.name = "software.avc".into();
        software.hardware = Some(false);
        report.details.push(software);
        snapshot.decoders = Some(report);
        let stream = blent_config::negotiation::StreamProfile {
            codec: "h264".into(),
            format: blent_config::negotiation::Profile {
                profile: "baseline".into(),
                level: 31,
                depth: 8,
            },
        };
        let mut best = measured("libx264", 20_000);
        best.measurement.stream = Some(stream.clone());
        best.decoder = Some(snapshot.decoders.as_ref().unwrap().choices(&stream, true)[0].clone());
        let alternatives = variants(&snapshot, &best);
        assert_eq!(alternatives.len(), 2);
        assert!(alternatives.iter().all(|c| c.observation.is_none()));
        assert!(alternatives[0].hardware);
        assert!(!alternatives[1].hardware);
        let unhinted = alternatives[0].decoder.as_ref().unwrap();
        assert!(!unhinted.low_latency);
        assert_eq!(unhinted.operating_rate, None);
        let (tx, _rx) = watch::channel(snapshot.clone());
        let mut trials = 0;
        let rows = benchmark_with(&tx, &snapshot, vec![best.clone()], |_| {
            trials += 1;
            std::future::ready(best.observation.clone())
        })
        .await;
        assert_eq!(trials, 3);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|c| c.observation.is_some()));
    }

    fn measured(name: &str, latency: u32) -> Candidate {
        let mut row = candidate(name, true, 120.0, 1);
        row.observation = Some(Observation {
            resources: Some(crate::selection::resources::Usage { cpu_us_per_frame: 1000.0, peak_rss_bytes: 100_000 }),
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
