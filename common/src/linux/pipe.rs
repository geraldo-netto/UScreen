//! Host-owned raw pipe policy. Publishing a request never opens the capture FIFO.
use anyhow::{Context, Result};
use std::{io::Write, path::Path};

pub fn request_path() -> Result<std::path::PathBuf> {
    Ok(super::runtime::runtime_dir()?.join("pipe-capacity-mib"))
}

/// Startup must read the latest preference inside the publication transaction.
pub fn publish_current() -> Result<()> {
    publish_current_at(&crate::storage::ConfigStore::default(), &request_path()?)
}

pub fn publish_current_at(store: &crate::storage::ConfigStore, path: &Path) -> Result<()> {
    store.read_locked(|config| publish_at(path, config.pipe_capacity_mib))
}

pub fn publish(mib: u32) -> Result<()> {
    publish_at(&request_path()?, mib)
}

pub fn publish_at(path: &Path, mib: u32) -> Result<()> {
    anyhow::ensure!(
        crate::model::PIPE_CAPACITIES_MIB.contains(&mib),
        "Invalid pipe capacity"
    );
    let parent = path.parent().context("pipe request has no parent")?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(staged, "{mib}")?;
    staged.as_file().sync_all()?;
    staged.persist(path).context("publish pipe capacity")?;
    Ok(())
}

mod report;
pub use report::{effective_bytes, ReportFile};

pub fn ceiling_bytes() -> Option<u32> {
    std::fs::read_to_string("/proc/sys/fs/pipe-max-size")
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::FileConfig, storage::ConfigStore};
    use std::fs::OpenOptions;
    use std::os::unix::fs::PermissionsExt;
    use std::os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    };

    #[test]
    fn t463_startup_reads_latest_preference_instead_of_old_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("config.toml"));
        let old = store.load();
        let edited = FileConfig {
            pipe_capacity_mib: 4,
            ..old.clone()
        };
        store.save_edits(&edited, &old).unwrap();
        let request = dir.path().join("request");
        publish_current_at(&store, &request).unwrap();
        assert_eq!(std::fs::read_to_string(request).unwrap(), "4\n");
    }

    #[test]
    fn t463_save_waits_for_startup_publication_before_publishing_new_value() {
        use std::sync::mpsc;
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("config.toml"));
        let request = dir.path().join("request");
        let startup_store = store.clone();
        let startup_path = request.clone();
        let (entered, waiting) = mpsc::channel();
        let (release, paused) = mpsc::channel();
        let startup = std::thread::spawn(move || {
            startup_store.read_locked(|config| {
                entered.send(()).unwrap();
                paused.recv_timeout(Duration::from_secs(5)).unwrap();
                publish_at(&startup_path, config.pipe_capacity_mib)
            })
        });
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        let save_store = store.clone();
        let save_path = request.clone();
        let (published, completion) = mpsc::channel();
        let save = std::thread::spawn(move || {
            let baseline = FileConfig::default();
            let edited = FileConfig {
                pipe_capacity_mib: 8,
                ..baseline.clone()
            };
            save_store
                .save_edits_then(&edited, &baseline, |config| {
                    publish_at(&save_path, config.pipe_capacity_mib).unwrap();
                    published.send(()).unwrap();
                })
                .unwrap();
        });
        let overtook = completion.recv_timeout(Duration::from_millis(250)).is_ok();
        release.send(()).unwrap();
        startup.join().unwrap().unwrap();
        save.join().unwrap();
        assert!(!overtook, "T463: save overtook startup publication");
        assert_eq!(store.load().pipe_capacity_mib, 8);
        assert_eq!(std::fs::read_to_string(request).unwrap(), "8\n");
    }

    #[test]
    fn t463_failed_publication_keeps_preference_for_startup_retry() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("config.toml"));
        let baseline = store.load();
        let edited = FileConfig {
            pipe_capacity_mib: 2,
            ..baseline.clone()
        };
        let request = dir.path().join("missing/request");
        let (saved, publication) = store
            .save_edits_then(&edited, &baseline, |config| {
                publish_at(&request, config.pipe_capacity_mib)
            })
            .unwrap();
        assert!(publication.is_err());
        assert_eq!(saved.pipe_capacity_mib, 2);
        assert_eq!(store.load(), saved);
        std::fs::create_dir(request.parent().unwrap()).unwrap();
        publish_current_at(&store, &request).unwrap();
        assert_eq!(std::fs::read_to_string(request).unwrap(), "2\n");
    }

    #[test]
    fn t415_status_does_not_wake_a_reader_waiting_for_the_capture_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frames");
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let input = path.clone();
        let reader = std::thread::spawn(move || {
            let _fd = std::fs::File::open(input).unwrap();
            tx.send(()).unwrap();
        });
        let mut awakened = false;
        for _ in 0..20 {
            effective_bytes(&path);
            if rx.recv_timeout(std::time::Duration::from_millis(5)).is_ok() {
                awakened = true;
                break;
            }
        }
        let _cleanup = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path);
        reader.join().unwrap();
        assert!(
            !awakened,
            "T415: checking status must not admit a waiting encoder reader"
        );
    }

    #[test]
    fn t415_requests_are_atomic_private_validated_and_do_not_follow_links() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request");
        let other = dir.path().join("other");
        std::fs::write(&other, "unchanged").unwrap();
        std::os::unix::fs::symlink(&other, &path).unwrap();
        for mib in crate::model::PIPE_CAPACITIES_MIB {
            publish_at(&path, mib).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{mib}\n"));
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(publish_at(&path, 3).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "8\n");
        assert_eq!(std::fs::read_to_string(other).unwrap(), "unchanged");
    }

    #[test]
    fn t415_inspection_never_consumes_frames_or_opens_a_reader() {
        use std::{ffi::CString, io::Read};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frames");
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert_eq!(effective_bytes(&path), None);
        let mut reader = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .unwrap();
        let mut writer = OpenOptions::new().write(true).open(&path).unwrap();
        writer.write_all(b"frame").unwrap();
        assert_eq!(effective_bytes(&path), None);
        let status = ReportFile::new(path.clone(), std::process::id()).unwrap();
        let metadata = writer.metadata().unwrap();
        let bytes = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETPIPE_SZ) };
        status
            .observe(&format!(
                "PIPE_CAPACITY 1048576 {bytes} 0 {} {}",
                metadata.dev(),
                metadata.ino()
            ))
            .unwrap();
        assert_eq!(
            effective_bytes(&path),
            Some(unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETPIPE_SZ) } as u32)
        );
        let mut bytes = [0; 5];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"frame");
        status.observe("PIPE_CAPACITY 0 0 0 0 0").unwrap();
        drop(writer);
        drop(reader);
        assert_eq!(effective_bytes(&path), None);
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(effective_bytes(&link), None);
    }
}
