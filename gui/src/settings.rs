//! One owned save at a time. File locks/fsync and an optional restart run off
//! the render thread; completion carries the exact submitted/saved snapshots.
use std::thread::JoinHandle;
use uscreen_config::{model::FileConfig, storage::ConfigStore};

pub type PipeApply = Box<dyn FnOnce(u32) -> Result<(), String> + Send>;

pub type Restart = Box<dyn FnOnce() -> Result<(), String> + Send>;

pub fn apply_label(running: bool, edited: &FileConfig, previous: &FileConfig) -> &'static str {
    if !running {
        "Save"
    } else if edited.requires_restart_from(previous) {
        "Apply & restart"
    } else {
        "Apply"
    }
}

pub struct Saved {
    pub config: FileConfig,
    pub restart: Option<Result<(), String>>,
    pub pipe: Option<Result<(), String>>,
}

impl Saved {
    pub fn message(&self) -> String {
        if let Some(Err(error)) = &self.pipe {
            return format!("Settings saved — pipe request failed: {error}");
        }
        match &self.restart {
            None if self.pipe.is_some() => {
                "Settings saved — pipe size requested; see effective capacity below the selector"
                    .into()
            }
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
        Self::start_with_pipe(
            store,
            submitted,
            baseline,
            restart,
            Box::new(|mib| uscreen_config::linux::pipe::publish(mib).map_err(|e| e.to_string())),
        )
    }

    fn start_with_pipe(
        store: ConfigStore,
        submitted: FileConfig,
        baseline: FileConfig,
        restart: Option<Restart>,
        apply_pipe: PipeApply,
    ) -> Self {
        let edited = submitted.clone();
        let worker = std::thread::spawn(move || {
            let config = store
                .save_edits(&edited, &baseline)
                .map_err(|error| format!("Save failed: {error}"))?;
            let pipe = (config.pipe_capacity_mib != baseline.pipe_capacity_mib)
                .then(|| apply_pipe(config.pipe_capacity_mib));
            // Exactly one restart, only after the transaction has committed.
            let restart = restart.map(|action| action());
            Ok(Saved {
                config,
                restart,
                pipe,
            })
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
    fn t332_invalid_display_mode_cannot_save_or_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("config.toml"));
        let saved = store
            .update(|cfg| {
                cfg.width = 3840;
                cfg.height = 2160;
                cfg.fps = 60;
                Ok(())
            })
            .unwrap();
        let edited = FileConfig {
            fps: 90,
            ..saved.clone()
        };
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = count.clone();
        let restart: Restart = Box::new(move || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        let result =
            PendingSave::start(store.clone(), edited, saved.clone(), Some(restart)).finish();
        assert!(result.is_err(), "T332: invalid display mode accepted");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(store.load(), saved);
    }

    #[test]
    fn t415_live_pipe_save_persists_then_publishes_without_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("config.toml"));
        let mut baseline = FileConfig::default();
        for mib in [2, 4, 8, 1] {
            let edited = FileConfig {
                pipe_capacity_mib: mib,
                ..baseline.clone()
            };
            assert!(!edited.requires_restart_from(&baseline));
            assert_eq!(apply_label(true, &edited, &baseline), "Apply");
            let reader = store.clone();
            let path = dir.path().join("request");
            let request = path.clone();
            let publish: PipeApply = Box::new(move |value| {
                assert_eq!(
                    reader.load().pipe_capacity_mib,
                    value,
                    "T415: publish follows commit"
                );
                uscreen_config::linux::pipe::publish_at(&request, value).map_err(|e| e.to_string())
            });
            let saved =
                PendingSave::start_with_pipe(store.clone(), edited, baseline, None, publish)
                    .finish()
                    .unwrap();
            assert!(saved.restart.is_none());
            assert!(saved.pipe.unwrap().is_ok());
            assert_eq!(std::fs::read_to_string(path).unwrap(), format!("{mib}\n"));
            baseline = saved.config;
        }
        let mixed = FileConfig {
            fps: 30,
            pipe_capacity_mib: 8,
            ..baseline.clone()
        };
        assert!(mixed.requires_restart_from(&baseline));
        assert_eq!(apply_label(true, &mixed, &baseline), "Apply & restart");
    }

    #[test]
    fn t415_publish_failure_does_not_lose_saved_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("config.toml"));
        let baseline = FileConfig::default();
        let edited = FileConfig {
            pipe_capacity_mib: 8,
            ..baseline.clone()
        };
        let saved = PendingSave::start_with_pipe(
            store.clone(),
            edited,
            baseline,
            None,
            Box::new(|_| Err("runtime unavailable".into())),
        )
        .finish()
        .unwrap();
        assert_eq!(store.load().pipe_capacity_mib, 8);
        assert!(saved.message().contains("pipe request failed"));
    }

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
