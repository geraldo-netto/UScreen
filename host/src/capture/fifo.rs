//! A partial raw frame retires its FIFO inode, not merely its write descriptor.
use anyhow::{Context, Result};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    pub(super) fn from_reset_line(line: &str) -> Option<Self> {
        let mut words = line.strip_prefix("FIFO_RESET ")?.split_whitespace();
        let id = Self {
            device: words.next()?.parse().ok()?,
            inode: words.next()?.parse().ok()?,
        };
        words.next().is_none().then_some(id)
    }

    fn at(path: &Path) -> Result<Self> {
        let meta = std::fs::symlink_metadata(path).context("stat capture FIFO")?;
        anyhow::ensure!(meta.file_type().is_fifo(), "capture path is not a FIFO");
        Ok(Self {
            device: meta.dev(),
            inode: meta.ino(),
        })
    }

    pub(super) fn matches(self, path: &Path) -> Result<bool> {
        Ok(Self::at(path)? == self)
    }
}

/// Caller has stopped and reaped the old reader. Allocate before unlinking the
/// old FIFO so inode reuse cannot authorize a write to the retired stream.
pub(super) fn replace_retired(path: &Path, retired: Identity) -> Result<()> {
    if !retired.matches(path)? {
        return Ok(());
    }
    let parent = path.parent().context("capture FIFO has no parent")?;
    let staging = tempfile::Builder::new()
        .prefix("fifo-reset-")
        .tempdir_in(parent)?;
    let replacement = staging.path().join("frames");
    super::helper::ensure_fifo(&replacement)?;
    anyhow::ensure!(
        !retired.matches(&replacement)?,
        "replacement reused the retired FIFO"
    );
    if retired.matches(path)? {
        std::fs::rename(&replacement, path).context("replace retired capture FIFO")?;
    }
    Ok(())
}
