//! A partial raw frame retires its FIFO inode, not merely its write descriptor.
use anyhow::{Context, Result};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

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
        Self::from_metadata(&meta)
    }

    fn from_metadata(meta: &std::fs::Metadata) -> Result<Self> {
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

/// Own a particular inode, not a slot name. O_PATH pins identity without opening
/// a reader/writer endpoint or affecting FIFO connection and EOF semantics.
pub(super) struct Owned {
    path: PathBuf,
    identity: Identity,
    _inode: std::fs::File,
}

impl Owned {
    pub(super) fn create(path: &Path) -> Result<Self> {
        super::helper::ensure_fifo(path)?;
        let inode = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)
            .open(path)
            .context("pin owned capture FIFO")?;
        let identity = Identity::from_metadata(&inode.metadata()?)?;
        Ok(Self {
            path: path.into(),
            identity,
            _inode: inode,
        })
    }

    /// Caller has retired the old reader. Allocate before replacing the FIFO
    /// so an old writer/reset report cannot authorize writes to the fresh one.
    pub(super) fn replace_retired(&mut self, retired: Identity) -> Result<()> {
        if retired != self.identity || !retired.matches(&self.path)? {
            return Ok(());
        }
        let parent = self.path.parent().context("capture FIFO has no parent")?;
        let staging = tempfile::Builder::new()
            .prefix("fifo-reset-")
            .tempdir_in(parent)?;
        let mut replacement = Self::create(&staging.path().join("frames"))?;
        anyhow::ensure!(
            replacement.identity != retired,
            "replacement reused the retired FIFO"
        );
        if retired.matches(&self.path)? {
            std::fs::rename(&replacement.path, &self.path)
                .context("replace retired capture FIFO")?;
            replacement.path = self.path.clone();
            *self = replacement;
        }
        Ok(())
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if self.identity.matches(&self.path).unwrap_or(false) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
