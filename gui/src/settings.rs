//! One owned save at a time. File locks/fsync and an optional restart run off
//! the render thread; completion carries the exact submitted/saved snapshots.
use std::thread::JoinHandle;
use uscreen_config::{model::FileConfig, storage::ConfigStore};

pub type Restart = Box<dyn FnOnce() -> Result<(), String> + Send>;

pub struct Saved {
    pub config: FileConfig,
    pub restart: Option<Result<(), String>>,
}

impl Saved {
    pub fn message(&self) -> String {
        match &self.restart {
            None => "Settings saved".into(),
            Some(Ok(())) => "Settings saved — daemon restarted".into(),
            Some(Err(error)) => format!("Settings saved — restart failed: {error}"),
        }
    }
}

pub struct PendingSave {
    pub submitted: FileConfig,
    worker: JoinHandle<Result<Saved, String>>,
}

// Dropping the window does not synchronously join a file-lock wait. A started
// transaction cannot be rolled back by dropping its handle; the process may
// terminate it on exit, and atomic file replacement preserves the old/new
// config boundary. While the UI lives, it owns and collects this single job.

impl PendingSave {
    pub fn start(
        store: ConfigStore,
        submitted: FileConfig,
        baseline: FileConfig,
        restart: Option<Restart>,
    ) -> Self {
        let edited = submitted.clone();
        let worker = std::thread::spawn(move || {
            let config = store
                .save_edits(&edited, &baseline)
                .map_err(|error| format!("Save failed: {error}"))?;
            // Exactly one restart, only after the transaction has committed.
            let restart = restart.map(|action| action());
            Ok(Saved { config, restart })
        });
        Self { submitted, worker }
    }

    pub fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    pub fn finish(self) -> Result<Saved, String> {
        self.worker
            .join()
            .map_err(|_| "Save worker failed".to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn t378_restart_runs_once_after_commit_and_never_after_save_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = ConfigStore::new(path.clone());
        let count = Arc::new(AtomicUsize::new(0));
        for fail in [true, false] {
            let baseline = FileConfig::default();
            std::fs::write(&path, if fail { "invalid = [" } else { "" }).unwrap();
            let edited = FileConfig {
                fps: 30,
                ..baseline.clone()
            };
            let counter = count.clone();
            let readback = store.clone();
            let restart: Restart = Box::new(move || {
                assert_eq!(readback.load().fps, 30, "T378: restart precedes commit");
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
            let result =
                PendingSave::start(store.clone(), edited, baseline, Some(restart)).finish();
            assert_eq!(result.is_err(), fail);
            assert_eq!(count.load(Ordering::SeqCst), usize::from(!fail));
        }
    }
}
