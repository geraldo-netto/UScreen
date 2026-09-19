use super::*;
use crate::attachment::Attachment;

enum Prepared {
    Cached(Box<Candidate>),
    Measured(Vec<Candidate>),
}

pub(super) async fn optimize(
    base: &CaptureConfig,
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    latency: &LatencyTracker,
    attachment: &Attachment,
) {
    let cache = cache::Cache::open(base, snapshot, attachment).await;
    optimize_with(&HostProbes(base), settings, snapshot, latency, cache).await;
}

trait Probes {
    async fn candidates(&self, snapshot: &EncoderSettings) -> Vec<Candidate>;
}
struct HostProbes<'a>(&'a CaptureConfig);
impl Probes for HostProbes<'_> {
    async fn candidates(&self, snapshot: &EncoderSettings) -> Vec<Candidate> {
        calibrate(self.0, snapshot).await
    }
}

async fn optimize_with(
    probes: &impl Probes,
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    latency: &LatencyTracker,
    cache: Option<Arc<cache::Cache>>,
) {
    let mut reuse = cache.as_deref();
    loop {
        let Some(prepared) = prepare(probes, settings, snapshot, latency, reuse).await else {
            publish(
                settings,
                &Key::new(snapshot),
                fallback_encoder(snapshot),
                "Calibration interrupted; restored fallback",
            );
            return;
        };
        match prepared {
            Prepared::Measured(candidates) => {
                supervise(settings, snapshot, latency, candidates, cache.clone()).await;
                return;
            }
            Prepared::Cached(candidate) => {
                watch_cached(reuse.unwrap(), latency, &Key::new(snapshot), &candidate).await;
                publish(
                    settings,
                    &Key::new(snapshot),
                    fallback_encoder(snapshot),
                    "Historical profile failed; measuring current candidates",
                );
                reuse = None;
            }
        }
    }
}

async fn prepare(
    probes: &impl Probes,
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    latency: &LatencyTracker,
    cache: Option<&cache::Cache>,
) -> Option<Prepared> {
    let rich = snapshot.decoders.as_ref().is_some_and(|d| d.protocol == 2);
    let mut input = latency.interaction_updates();
    if rich && !super::super::trial::quiet(&mut input).await {
        return None;
    }
    let work = async {
        let _permit = tokio::time::timeout(Duration::from_secs(3), ADMISSION.acquire())
            .await
            .ok()?
            .ok()?;
        let candidates = probes.candidates(snapshot).await;
        if let Some(candidate) = cached(settings, snapshot, latency, cache, &candidates).await {
            return Some(Prepared::Cached(Box::new(candidate)));
        }
        let candidates = if rich {
            measured::benchmark(settings, snapshot, latency, candidates).await
        } else {
            candidates
        };
        Some(Prepared::Measured(candidates))
    };
    if rich {
        super::super::trial::uninterrupted(&mut input, work)
            .await
            .flatten()
    } else {
        work.await
    }
}

async fn cached(
    settings: &watch::Sender<EncoderSettings>,
    snapshot: &EncoderSettings,
    latency: &LatencyTracker,
    cache: Option<&cache::Cache>,
    candidates: &[Candidate],
) -> Option<Candidate> {
    let cache = cache?;
    let candidate = cache.load(cache::now(), snapshot, candidates)?;
    let key = Key::new(snapshot);
    let choice = candidate.decoder.clone();
    let mut retirement = Box::pin(cache.retired());
    let result = tokio::select! {
        _ = &mut retirement => None,
        result = choose(settings, &key, fallback_encoder(snapshot), vec![candidate], |name| {
            let key = &key; let choice = choice.as_ref();
            async move { rendered(latency, key, &name, choice).await }
        }) => result,
    };
    if let Some((candidate, _)) = result {
        return Some(candidate);
    }
    cache.invalidate();
    None
}

async fn watch_cached(
    cache: &cache::Cache,
    latency: &LatencyTracker,
    key: &Key,
    candidate: &Candidate,
) {
    tokio::select! {
        _ = cache.retired() => {},
        _ = super::super::health::failed(latency, key, &candidate.measurement.encoder, candidate.decoder.as_ref()) => {},
    }
    cache.invalidate();
}

#[cfg(test)]
mod tests;
