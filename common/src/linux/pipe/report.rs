//! Writer reports preserve FIFO startup/EOF semantics; readers never open it.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Snapshot {
    pid: u32,
    start_ticks: u64,
    requested: u32,
    effective: u32,
    error: u32,
    device: u64,
    inode: u64,
}

fn load(path: &Path) -> Option<Snapshot> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

pub fn effective_bytes(fifo: &Path) -> Option<u32> {
    let report = load(&fifo.with_extension("pipe-status"))?;
    if report.effective == 0
        || super::super::processes::start_ticks(report.pid) != Some(report.start_ticks)
    {
        return None;
    }
    let metadata = std::fs::symlink_metadata(fifo).ok()?;
    (metadata.file_type().is_fifo()
        && metadata.dev() == report.device
        && metadata.ino() == report.inode)
        .then_some(report.effective)
}

pub struct ReportFile {
    path: PathBuf,
    pid: u32,
    start_ticks: u64,
}
impl ReportFile {
    pub fn new(fifo: PathBuf, pid: u32) -> Result<Self> {
        let path = fifo.with_extension("pipe-status");
        let start_ticks =
            super::super::processes::start_ticks(pid).context("read capture helper identity")?;
        let _ = std::fs::remove_file(&path);
        Ok(Self {
            path,
            pid,
            start_ticks,
        })
    }
    pub fn observe(&self, line: &str) -> Result<()> {
        let Some(values) = line.strip_prefix("PIPE_CAPACITY ") else {
            return Ok(());
        };
        let values: Vec<u64> = values
            .split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()?;
        anyhow::ensure!(values.len() == 5, "Invalid pipe status field count");
        let snapshot = self.snapshot(&values)?;
        let parent = self.path.parent().context("pipe status has no parent")?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(&serde_json::to_vec(&snapshot)?)?;
        staged.persist(&self.path).context("publish pipe status")?;
        Ok(())
    }
    fn snapshot(&self, values: &[u64]) -> Result<Snapshot> {
        let requested = u32::try_from(values[0])?;
        anyhow::ensure!(
            requested == 0
                || crate::model::PIPE_CAPACITIES_MIB
                    .iter()
                    .any(|mib| mib * 1048576 == requested),
            "Invalid requested pipe size"
        );
        Ok(Snapshot {
            pid: self.pid,
            start_ticks: self.start_ticks,
            requested,
            effective: u32::try_from(values[1])?,
            error: u32::try_from(values[2])?,
            device: values[3],
            inode: values[4],
        })
    }
}
impl Drop for ReportFile {
    fn drop(&mut self) {
        if load(&self.path).is_some_and(|s| s.pid == self.pid && s.start_ticks == self.start_ticks)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t415_status_rejects_stale_processes_replaced_fifos_and_malformed_reports() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("frames.fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let metadata = std::fs::symlink_metadata(&fifo).unwrap();
        let report = ReportFile::new(fifo.clone(), std::process::id()).unwrap();
        let line = format!(
            "PIPE_CAPACITY 8388608 1048576 1 {} {}",
            metadata.dev(),
            metadata.ino()
        );
        report.observe(&line).unwrap();
        assert_eq!(effective_bytes(&fifo), Some(1048576));
        for invalid in [
            "PIPE_CAPACITY 1",
            "PIPE_CAPACITY bad data",
            "PIPE_CAPACITY 3 1048576 0 1 2",
        ] {
            assert!(report.observe(invalid).is_err());
            assert_eq!(effective_bytes(&fifo), Some(1048576));
        }
        let mut stale = load(&report.path).unwrap();
        stale.start_ticks += 1;
        std::fs::write(&report.path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert_eq!(effective_bytes(&fifo), None);
        report.observe(&line).unwrap();
        // Allocate the replacement before deleting the old FIFO; inode reuse
        // must not make this stale report look like the current stream.
        let next = dir.path().join("next");
        let name = std::ffi::CString::new(next.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        std::fs::rename(next, &fifo).unwrap();
        assert_eq!(effective_bytes(&fifo), None);
        let status_path = report.path.clone();
        drop(report);
        assert!(!status_path.exists());
    }
}
