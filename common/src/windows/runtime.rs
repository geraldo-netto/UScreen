//! Windows private state and single-instance ownership, independent of capture.
use super::{private::Directory, process::Identity};
use anyhow::{Context, Result};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};

pub fn directory() -> Result<Directory> {
    Directory::create(&super::paths::local()?.join("uscreen"))
}

pub fn runtime_dir() -> Result<PathBuf> {
    Ok(directory()?.path().to_owned())
}

pub fn new_session_token() -> Result<String> {
    let directory = directory()?;
    let mut bytes = [0u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    anyhow::ensure!(status >= 0, "Windows random source failed ({status:#x})");
    let token = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut file = tempfile::NamedTempFile::new_in(directory.path())?;
    file.write_all(token.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(directory.path().join("token"))?;
    Ok(token)
}

pub struct Lease {
    directory: Directory,
    _lock: std::fs::File,
    identity: Identity,
}

impl Lease {
    pub fn acquire(directory: Directory) -> Result<Self> {
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.path().join("daemon.lock"))?;
        lock.try_lock()
            .context("another UScreen instance owns this runtime")?;
        let identity = Identity::read(std::process::id())?;
        let mut record = tempfile::NamedTempFile::new_in(directory.path())?;
        serde_json::to_writer(&mut record, &identity)?;
        record.as_file().sync_all()?;
        record.persist(directory.path().join("daemon.json"))?;
        Ok(Self {
            directory,
            _lock: lock,
            identity,
        })
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Retire our record before releasing the stable lock-file handle.
        let path = self.directory.path().join("daemon.json");
        if read_owner(&path).as_ref() == Some(&self.identity) {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn owner_at(directory: &Directory) -> Option<Identity> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.path().join("daemon.lock"))
        .ok()?;
    match file.try_lock() {
        Ok(()) => None,
        Err(std::fs::TryLockError::WouldBlock) => read_owner(&directory.path().join("daemon.json")),
        Err(_) => None,
    }
}

fn read_owner(path: &Path) -> Option<Identity> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 65536 {
        return None;
    }
    let identity: Identity = serde_json::from_slice(&bytes).ok()?;
    identity.is_current().then_some(identity)
}
