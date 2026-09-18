//! Filesystem adapter for transactional configuration persistence.
use crate::model::FileConfig;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
}

pub fn config_path() -> PathBuf {
    config_home().join("uscreen/config.toml")
}

/// A filesystem adapter with an explicit location, shared by UI and daemon
/// workers. Keep synchronous transactions off their event/render threads.
#[derive(Clone, Debug)]
pub struct ConfigStore {
    path: PathBuf,
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new(config_path())
    }
}

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> FileConfig {
        FileConfig::load_at(&self.path)
    }

    pub fn update(&self, edit: impl FnOnce(&mut FileConfig) -> Result<()>) -> Result<FileConfig> {
        FileConfig::update_at(&self.path, edit)
    }

    /// Cooperative cancellation while waiting for a lock. Once filesystem
    /// commit begins, callers must await completion rather than assume abort
    /// undoes a write/fsync already in progress.
    pub fn update_cancellable(
        &self,
        cancelled: &std::sync::atomic::AtomicBool,
        edit: impl FnOnce(&mut FileConfig) -> Result<()>,
    ) -> Result<FileConfig> {
        use std::sync::atomic::Ordering;
        let lock = FileConfig::open_lock_at(&self.path)?;
        while !cancelled.load(Ordering::Acquire) {
            match lock.try_lock() {
                Ok(()) => {
                    anyhow::ensure!(!cancelled.load(Ordering::Acquire), "config save cancelled");
                    return FileConfig::update_locked_at(&self.path, edit);
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        anyhow::bail!("config save cancelled")
    }

    pub fn save_edits(&self, edited: &FileConfig, baseline: &FileConfig) -> Result<FileConfig> {
        self.save_edits_then(edited, baseline, |_| ())
            .map(|(config, ())| config)
    }

    /// Keep the cross-process lock through publication; release it before any
    /// daemon restart. Publication may fail after commit, so retain both results.
    pub fn save_edits_then<T>(
        &self,
        edited: &FileConfig,
        baseline: &FileConfig,
        publish: impl FnOnce(&FileConfig) -> T,
    ) -> Result<(FileConfig, T)> {
        let _lock = FileConfig::lock_at(&self.path)?;
        let config = FileConfig::update_locked_at(&self.path, |latest| {
            *latest = edited.merge_edits(baseline, latest.clone())?;
            Ok(())
        })?;
        let result = publish(&config);
        Ok((config, result))
    }

    /// Read and act on the current persisted snapshot under the same lock used
    /// by saves. The callback must not recursively start a config transaction.
    pub fn read_locked<T>(&self, action: impl FnOnce(&FileConfig) -> Result<T>) -> Result<T> {
        let _lock = FileConfig::lock_at(&self.path)?;
        action(&self.load())
    }
}

impl FileConfig {
    pub fn load() -> Self {
        Self::load_at(&config_path())
    }

    pub fn load_at(path: &Path) -> Self {
        let mut cfg = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("Invalid config at {:?}: {} — using defaults", path, e);
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        cfg.sanitize();
        cfg
    }

    pub fn save(&self) -> Result<()> {
        self.save_at(&config_path())
    }

    /// Serialize the entire read/modify/write operation across processes.
    pub fn update(edit: impl FnOnce(&mut Self) -> Result<()>) -> Result<Self> {
        ConfigStore::default().update(edit)
    }

    pub fn save_edits(&self, baseline: &Self) -> Result<Self> {
        ConfigStore::default().save_edits(self, baseline)
    }

    fn lock_at(path: &Path) -> Result<std::fs::File> {
        let lock = Self::open_lock_at(path)?;
        lock.lock().context("lock config transaction")?;
        Ok(lock)
    }

    fn open_lock_at(path: &Path) -> Result<std::fs::File> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))?;
        Ok(lock)
    }

    fn update_at(path: &Path, edit: impl FnOnce(&mut Self) -> Result<()>) -> Result<Self> {
        let _lock = Self::lock_at(path)?;
        Self::update_locked_at(path, edit)
    }

    fn update_locked_at(path: &Path, edit: impl FnOnce(&mut Self) -> Result<()>) -> Result<Self> {
        // A partial edit must never replace unreadable preferences with defaults.
        // Only a genuinely absent file starts a new configuration.
        let mut config: Self = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).context("parse config before update")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error).context("read config before update"),
        };
        config.sanitize();
        edit(&mut config)?;
        config.sanitize();
        config.write_at(path)?;
        Ok(config)
    }

    fn save_at(&self, path: &Path) -> Result<()> {
        let _lock = Self::lock_at(path)?;
        self.write_at(path)
    }

    fn write_at(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).context("serialize config")?;
        // Say exactly which lines change. Settings that drift with nobody
        // touching them are impossible to chase down otherwise.
        if let Ok(old) = std::fs::read_to_string(path) {
            let before: std::collections::BTreeMap<&str, &str> =
                old.lines().filter_map(|l| l.split_once(" = ")).collect();
            let changed: Vec<String> = text
                .lines()
                .filter_map(|l| l.split_once(" = "))
                .filter(|(k, v)| before.get(k) != Some(v))
                .map(|(k, v)| {
                    format!(
                        "{} = {} (was {})",
                        k,
                        v,
                        before.get(k).unwrap_or(&"<unset>")
                    )
                })
                .collect();
            if !changed.is_empty() {
                tracing::info!("Config written: {}", changed.join(", "));
            }
        }
        // Write-then-rename: a reader must never see a half-written file.
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().context("config directory")?)?;
        tmp.write_all(text.as_bytes())
            .context("write config file")?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).context("replace config file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t332_invalid_joint_mode_save_preserves_last_valid_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = ConfigStore::new(path.clone());
        let saved = store
            .update(|cfg| {
                cfg.width = 3840;
                cfg.height = 2160;
                cfg.fps = 60;
                Ok(())
            })
            .unwrap();
        let previous = std::fs::read(&path).unwrap();
        let invalid = FileConfig {
            fps: 90,
            ..saved.clone()
        };
        assert!(
            store.save_edits(&invalid, &saved).is_err(),
            "T332: invalid clock was saved"
        );
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        assert_eq!(store.load(), saved);
    }

    #[test]
    fn t288_invalid_pen_mode_save_preserves_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = ConfigStore::new(path.clone());
        let saved = store
            .update(|cfg| {
                cfg.input_pen = false;
                Ok(())
            })
            .unwrap();
        let previous = std::fs::read(&path).unwrap();
        let invalid = FileConfig {
            pen_only: true,
            ..saved.clone()
        };
        assert!(
            store.save_edits(&invalid, &saved).is_err(),
            "T288: incompatible save must fail"
        );
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        for pen_only in [false, true] {
            let valid = store
                .update(|cfg| {
                    cfg.input_pen = true;
                    cfg.pen_only = pen_only;
                    Ok(())
                })
                .unwrap();
            assert_eq!(store.load(), valid);
        }
    }

    #[test]
    fn t264_position_aliases_round_trip_as_one_canonical_choice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for (input, expected) in [
            (" LEFT ", "left"),
            ("Above", "above"),
            (" top ", "above"),
            (" BELOW ", "below"),
            ("Bottom", "below"),
            (" right ", "right"),
            ("typo", "right"),
        ] {
            std::fs::write(&path, format!("position = {input:?}\n")).unwrap();
            let config = FileConfig::load_at(&path);
            // GUI and doctor consume this canonical value; runtime parsing
            // must select the same direction before and after persistence.
            assert_eq!(config.position, expected, "T264: {input:?}");
            assert_eq!(
                crate::Position::parse_or_default(&config.position),
                crate::Position::parse_or_default(input)
            );
            config.save_at(&path).unwrap();
            assert_eq!(FileConfig::load_at(&path).position, expected);
        }
    }
    #[test]
    fn t140_xdg_config_isolation() {
        if let Ok(mode) = std::env::var("USCREEN_T140_CHILD") {
            if mode == "absolute" {
                let expected = PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap())
                    .join("uscreen/config.toml");
                assert_eq!(
                    config_path(),
                    expected,
                    "config must stay inside test's XDG directory"
                );
                assert!(!FileConfig::load().check_updates);
                FileConfig::update(|config| {
                    config.fps = 30;
                    Ok(())
                })
                .unwrap();
                assert_eq!(FileConfig::load().fps, 30);
            } else {
                let expected = PathBuf::from(std::env::var_os("HOME").unwrap())
                    .join(".config/uscreen/config.toml");
                assert_eq!(config_path(), expected);
            }
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("uscreen")).unwrap();
        std::fs::write(
            temp.path().join("uscreen/config.toml"),
            "check_updates = false\n",
        )
        .unwrap();
        for (mode, value) in [
            ("absolute", temp.path()),
            ("relative", Path::new("relative-config")),
            ("empty", Path::new("")),
        ] {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage::tests::t140_xdg_config_isolation",
                    "--nocapture",
                ])
                .env("USCREEN_T140_CHILD", mode)
                .env("XDG_CONFIG_HOME", value)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{mode}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        assert_eq!(
            FileConfig::load_at(&temp.path().join("uscreen/config.toml")).fps,
            30
        );
    }

    #[test]
    fn t137_partial_updates_preserve_invalid_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "encoder = 'libx264'\nbitrate = broken\n";
        std::fs::write(&path, original).unwrap();
        let result = FileConfig::update_at(&path, |config| {
            config.fps = 30;
            Ok(())
        });
        assert!(result.is_err(), "invalid config must reject partial edits");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let mut edited = false;
        assert!(FileConfig::update_at(&path, |_| {
            edited = true;
            Ok(())
        })
        .is_err());
        assert!(!edited, "read failure must be detected before editing");
        assert!(path.is_dir());

        std::fs::remove_dir(&path).unwrap();
        let initialized = FileConfig::update_at(&path, |config| {
            config.fps = 30;
            Ok(())
        })
        .unwrap();
        assert_eq!(initialized.fps, 30);
        assert_eq!(FileConfig::load_at(&path).fps, 30);
    }

    // T104: concurrent transactions retain every edit; readers never see partial TOML.
    #[test]
    fn t104_concurrent_config_transactions() {
        let dir = std::env::temp_dir().join(format!("uscreen-t104-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        FileConfig::default().save_at(&path).unwrap();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let barrier = &barrier;
            let path = &path;
            for _ in 0..8 {
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..4 {
                        FileConfig::update_at(path, |config| {
                            let old = config.bitrate;
                            std::thread::sleep(std::time::Duration::from_millis(5));
                            config.bitrate = old + 100;
                            Ok(())
                        })
                        .unwrap();
                        let text = std::fs::read_to_string(path).unwrap();
                        toml::from_str::<FileConfig>(&text).unwrap();
                    }
                });
            }
        });
        assert_eq!(FileConfig::load_at(&path).bitrate, 23_200);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
