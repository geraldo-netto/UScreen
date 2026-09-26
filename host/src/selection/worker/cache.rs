//! One optional historical winner; fresh probes and render receipts remain mandatory.
use super::{Candidate, EncoderSettings};
use crate::selection::trial::Observation;
use blent_config::negotiation::DecoderChoice;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

mod context;
#[cfg(test)]
pub(super) mod tests;
const MAX_BYTES: u64 = 65536;
const MAX_AGE: u64 = 86400;

pub(super) struct Cache {
    path: PathBuf,
    fingerprint: String,
    lease: Option<crate::attachment::Lease>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    fingerprint: String,
    saved_at: u64,
    encoder: String,
    decoder: DecoderChoice,
    observation: Observation,
    quality_db: f64,
}

impl Cache {
    pub async fn remember(self: &std::sync::Arc<Self>, candidate: &Candidate) {
        let cache = self.clone();
        let candidate = candidate.clone();
        let result = tokio::task::spawn_blocking(move || cache.save(now(), &candidate)).await;
        if !matches!(result, Ok(Ok(()))) {
            tracing::debug!("Measured profile could not be persisted");
        }
    }

    pub fn active(&self) -> bool {
        self.lease.as_ref().is_none_or(|lease| lease.apply(|| {}))
    }

    fn read(&self) -> Option<Record> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
            .ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.len() > MAX_BYTES {
            return None;
        }
        let mut data = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut data).ok()?;
        if data.len() as u64 > MAX_BYTES {
            return None;
        }
        serde_json::from_slice(&data).ok()
    }

    pub fn load(
        &self,
        now: u64,
        snapshot: &EncoderSettings,
        fresh: &[Candidate],
    ) -> Option<Candidate> {
        if !self.active() {
            return None;
        }
        let record = self.read()?;
        if record.fingerprint != self.fingerprint || !record.valid(now) {
            return None;
        }
        let mut candidate = compatible(&record, snapshot, fresh)?;
        candidate.decoder = Some(record.decoder);
        candidate.observation = Some(record.observation);
        candidate.cached = true;
        Some(candidate)
    }

    pub fn save(&self, now: u64, candidate: &Candidate) -> anyhow::Result<()> {
        anyhow::ensure!(self.active(), "retired cache context");
        let record = Record {
            schema: 1,
            fingerprint: self.fingerprint.clone(),
            saved_at: now,
            encoder: candidate.measurement.encoder.clone(),
            decoder: candidate
                .decoder
                .clone()
                .ok_or_else(|| anyhow::anyhow!("missing decoder"))?,
            observation: candidate
                .observation
                .clone()
                .ok_or_else(|| anyhow::anyhow!("missing measurement"))?,
            quality_db: candidate.measurement.quality_db.unwrap_or(f64::NAN),
        };
        anyhow::ensure!(
            !candidate.cached && record.valid(now),
            "invalid measured profile"
        );
        self.write(&record)
    }

    fn write(&self, record: &Record) -> anyhow::Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing cache parent"))?;
        std::fs::create_dir_all(parent)?;
        let data = serde_json::to_vec(record)?;
        anyhow::ensure!(data.len() as u64 <= MAX_BYTES, "profile cache too large");
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&data)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path)?;
        Ok(())
    }

    pub fn invalidate(&self) {
        if self
            .read()
            .is_some_and(|record| record.fingerprint == self.fingerprint)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub async fn retired(&self) {
        if let Some(lease) = &self.lease {
            lease.retirement().retired().await;
        } else {
            std::future::pending::<()>().await;
        }
    }
}

impl Record {
    fn valid(&self, now: u64) -> bool {
        self.schema == 1
            && now
                .checked_sub(self.saved_at)
                .is_some_and(|age| age <= MAX_AGE)
            && self.decoder.stream.valid()
            && self.quality_db.is_finite()
            && (0.0..=200.0).contains(&self.quality_db)
            && observation_valid(&self.observation)
    }
}

fn observation_valid(value: &Observation) -> bool {
    let times =
        value.p50_us <= value.p95_us && value.p95_us <= value.p99_us && value.p99_us <= 6_000_000;
    times
        && (12..=4096).contains(&value.samples)
        && (900..=1000).contains(&value.delivery_permille)
        && value.ack_fps.is_finite()
        && (0.0..=1000.0).contains(&value.ack_fps)
        && value.ack_fps > 0.0
        && value.startup_us <= 6_000_000
}

fn compatible(
    record: &Record,
    snapshot: &EncoderSettings,
    fresh: &[Candidate],
) -> Option<Candidate> {
    if snapshot.encoder != "auto" || !snapshot.geometry_ready {
        return None;
    }
    let caps = snapshot.decoders.as_ref()?;
    let supported = supports_choice(caps, &record.decoder);
    if !supported {
        return None;
    }
    let reference = fresh
        .iter()
        .find(|c| c.measurement.encoder == "libx264")?
        .measurement
        .quality_db?;
    fresh
        .iter()
        .find(|c| {
            c.measurement.encoder == record.encoder
                && c.measurement.stream.as_ref() == Some(&record.decoder.stream)
                && super::measured::quality_capacity(c, reference, snapshot.fps)
        })
        .cloned()
}

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn supports_choice(
    caps: &blent_config::negotiation::DecoderCapabilities,
    expected: &DecoderChoice,
) -> bool {
    caps.choices(&expected.stream, true)
        .into_iter()
        .any(|choice| {
            let mut unhinted = choice.clone();
            unhinted.low_latency = false;
            unhinted.operating_rate = None;
            choice == *expected || unhinted == *expected
        })
}
