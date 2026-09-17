//! Status polling lifetime; in-flight probes retain their command deadlines.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::Duration;

pub struct StatusWorker {
    refresh: SyncSender<()>,
    stopped: Arc<AtomicBool>,
}

impl StatusWorker {
    pub fn start(mut poll: impl FnMut(bool) + Send + 'static, interval: Duration) -> Self {
        let (refresh, receiver) = mpsc::sync_channel(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        std::thread::spawn(move || {
            let mut force = true;
            while !worker_stopped.load(Ordering::Acquire) {
                poll(force);
                match receiver.recv_timeout(interval) {
                    Ok(()) => force = true,
                    Err(RecvTimeoutError::Timeout) => force = false,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self { refresh, stopped }
    }

    pub fn refresh(&self) {
        // One pending refresh is enough; UI actions never wait for a probe.
        let _ = self.refresh.try_send(());
    }
}

impl Drop for StatusWorker {
    fn drop(&mut self) {
        // Check cancellation even when a queued refresh was delivered, or the
        // worker has not yet started. Dropping the sender also wakes its wait.
        self.stopped.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn t409_dropping_owner_stops_polling_after_inflight_probe() {
        assert_drop_stops(false);
    }

    #[test]
    fn t409_pending_refresh_cannot_outlive_owner() {
        assert_drop_stops(true);
    }

    fn assert_drop_stops(queue_refresh: bool) {
        let (observed, probes) = mpsc::channel();
        let paused = Arc::new(Barrier::new(2));
        let resume = paused.clone();
        let worker = StatusWorker::start(
            move |_| {
                if observed.send(()).is_ok() {
                    resume.wait();
                }
            },
            Duration::from_millis(10),
        );
        probes.recv_timeout(Duration::from_secs(1)).unwrap();
        if queue_refresh {
            worker.refresh();
        }
        drop(worker);
        paused.wait();
        assert!(
            probes.recv_timeout(Duration::from_secs(1)).is_err(),
            "T409: dropped status owner started another probe"
        );
    }
}
