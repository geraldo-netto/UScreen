//! Linux capture adapter for portable T492 policy. Requests expire after 1.5s.
use super::CaptureConfig;
use crate::latency::{EncoderEvidence, LatencyTracker};
use anyhow::{Context, Result};
use blent_config::idle::{Phase, Policy, Sample, COMPATIBLE_MS};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::time::Instant;

const MAX_TIMINGS: usize = 256;
static LEASE_FILES: Mutex<()> = Mutex::new(());
#[derive(Clone, Copy)]
struct Encoded {
    sequence: u32,
    pts_us: Option<i64>,
    ready: Instant,
    keyframe: bool,
}
type Timings = Arc<Mutex<VecDeque<Encoded>>>;

pub(super) struct Handle {
    timings: Timings,
    lease: Arc<Lease>,
    task: tokio::task::JoinHandle<()>,
}
impl Handle {
    pub(super) fn note(&self, sequence: u32, pts_us: Option<i64>, keyframe: bool) {
        let mut timings = self.timings.lock().unwrap();
        timings.push_back(Encoded {
            sequence,
            pts_us,
            ready: Instant::now(),
            keyframe,
        });
        if timings.len() > MAX_TIMINGS {
            timings.pop_front();
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.lease.retire();
        self.task.abort();
    }
}

pub(super) fn start(
    config: &CaptureConfig,
    evidence: Arc<EncoderEvidence>,
    tracker: LatencyTracker,
    viewers: Arc<AtomicU64>,
) -> Option<Handle> {
    if !config.adaptive_idle || evidence.decoder.is_none() {
        return None;
    }
    let lease = match super::fifo_path_for(config.instance).and_then(|path| Lease::new(&path)) {
        Ok(lease) => lease,
        Err(error) => {
            tracing::warn!("Adaptive idle unavailable: {error}");
            return None;
        }
    };
    Some(launch(lease, evidence, tracker, viewers))
}

fn launch(
    lease: Lease,
    evidence: Arc<EncoderEvidence>,
    tracker: LatencyTracker,
    viewers: Arc<AtomicU64>,
) -> Handle {
    let lease = Arc::new(lease);
    let timings = Timings::default();
    let task = tokio::spawn(run(
        lease.clone(),
        timings.clone(),
        evidence,
        tracker,
        Instant::now(),
        viewers.clone(),
        viewers.load(Ordering::Acquire),
    ));
    Handle {
        timings,
        lease,
        task,
    }
}

async fn run(
    lease: Arc<Lease>,
    timings: Timings,
    evidence: Arc<EncoderEvidence>,
    tracker: LatencyTracker,
    origin: Instant,
    viewers: Arc<AtomicU64>,
    viewer_epoch: u64,
) {
    let mut updates = tracker.activity_updates();
    let mut timer = tokio::time::interval(Duration::from_millis(250));
    let mut ordinal = 0;
    let mut policy = Policy::new(evidence.decoder.is_some());
    let mut previous = policy.phase;
    while evidence.active() {
        if viewers.load(Ordering::Acquire) != viewer_epoch {
            lease.clear();
            return;
        }
        consume(&mut policy, &timings, &evidence, &mut ordinal, origin);
        policy.tick(micros(origin, Instant::now()));
        if policy.phase != previous {
            tracing::info!(
                "Adaptive idle: {:?}, encoder={}",
                policy.phase,
                evidence.name
            );
            previous = policy.phase;
        }
        if let Err(error) = lease.publish(policy.interval_ms()) {
            tracing::warn!("Adaptive idle restored compatibility: {error}");
            lease.clear();
            return;
        }
        if policy.phase == Phase::Rejected {
            return;
        }
        tokio::select! { _ = timer.tick() => {}, _ = updates.changed() => {} }
    }
    lease.clear();
}

fn consume(
    policy: &mut Policy,
    timings: &Timings,
    evidence: &EncoderEvidence,
    ordinal: &mut u64,
    origin: Instant,
) {
    for ack in evidence.samples_after(*ordinal) {
        if ack.ordinal != *ordinal + 1 {
            policy.reject();
            return;
        }
        *ordinal = ack.ordinal;
        let encoded = take(timings, ack.sequence);
        match encoded.and_then(|frame| {
            frame.pts_us.map(|pts_us| Sample {
                sequence: frame.sequence,
                pts_us,
                ready_us: micros(origin, frame.ready),
                ack_us: micros(origin, ack.at),
                keyframe: frame.keyframe,
            })
        }) {
            Some(sample) => policy.observe(sample),
            None => {
                policy.reject();
                return;
            }
        }
    }
}

fn take(timings: &Timings, sequence: u32) -> Option<Encoded> {
    let mut timings = timings.lock().unwrap();
    let position = timings
        .iter()
        .position(|frame| frame.sequence == sequence)?;
    timings.drain(..position);
    timings.pop_front()
}

fn micros(origin: Instant, at: Instant) -> u64 {
    at.saturating_duration_since(origin)
        .as_micros()
        .min(u64::MAX as u128) as u64
}

struct Lease {
    path: PathBuf,
    identity: (u64, u64),
    token: u64,
    retired: AtomicBool,
    _inode: std::fs::File,
    _token: std::fs::File,
}
impl Lease {
    fn new(fifo: &Path) -> Result<Self> {
        let inode = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(fifo)?;
        let metadata = inode.metadata()?;
        anyhow::ensure!(
            metadata.file_type().is_fifo(),
            "idle request needs an owned FIFO"
        );
        let token = tempfile::tempfile_in(fifo.parent().context("FIFO directory")?)?;
        Ok(Self {
            path: fifo.with_extension("idle"),
            identity: (metadata.dev(), metadata.ino()),
            token: token.metadata()?.ino(),
            retired: AtomicBool::new(false),
            _inode: inode,
            _token: token,
        })
    }

    fn publish(&self, interval: u32) -> Result<()> {
        if interval == COMPATIBLE_MS {
            self.clear();
            return Ok(());
        }
        let _guard = LEASE_FILES.lock().unwrap();
        anyhow::ensure!(!self.retired.load(Ordering::Relaxed), "idle lease retired");
        anyhow::ensure!(interval == 500, "unsupported sparse cadence");
        let mut clock = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // Linux CLOCK_MONOTONIC is also used by the helper; no wall-clock lease.
        anyhow::ensure!(
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut clock) } == 0,
            "monotonic clock unavailable"
        );
        let expires = clock.tv_sec as u64 * 1000 + clock.tv_nsec as u64 / 1_000_000 + 1500;
        let mut file =
            tempfile::NamedTempFile::new_in(self.path.parent().context("idle control directory")?)?;
        writeln!(
            file,
            "{} {} {} {expires} {interval}",
            self.identity.0, self.identity.1, self.token
        )?;
        file.persist(&self.path).context("publish idle request")?;
        Ok(())
    }

    fn contents(&self) -> std::io::Result<String> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(&self.path)?;
        let mut text = String::new();
        file.take(160).read_to_string(&mut text)?;
        Ok(text)
    }

    fn retire(&self) {
        let _guard = LEASE_FILES.lock().unwrap();
        self.retired.store(true, Ordering::Relaxed);
        self.clear_locked();
    }

    fn clear(&self) {
        let _guard = LEASE_FILES.lock().unwrap();
        self.clear_locked();
    }

    fn clear_locked(&self) {
        let prefix = format!("{} {} {} ", self.identity.0, self.identity.1, self.token);
        if self.contents().is_ok_and(|text| text.starts_with(&prefix)) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests;
