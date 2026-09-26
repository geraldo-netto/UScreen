//! Interactive-user daemon ownership. No display/input backend is implied.
use super::{
    private::Directory,
    process::Identity,
    runtime::{self, Lease},
};
use anyhow::{Context, Result};
use std::{
    io::{Read, Write},
    path::Path,
    time::Duration,
};

const STATE: [&str; 4] = ["ready.json", "stop.json", "sessions.json", "token"];

pub struct Session {
    lease: Lease,
}
impl Session {
    pub fn start(path: &Path) -> Result<Self> {
        let session = Self {
            lease: Lease::acquire(Directory::create(path)?)?,
        };
        clear(path)?;
        runtime::new_session_token(session.lease.private_directory())?;
        publish(path, "ready.json", session.lease.identity())?;
        Ok(session)
    }
    pub fn stop_requested(&self) -> bool {
        read(&self.lease.directory().join("stop.json")).as_ref() == Some(self.lease.identity())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // Never retire replacement state. The stable lease stays held throughout.
        if runtime::owner_at(self.lease.private_directory()).as_ref() == Some(self.lease.identity())
        {
            let _ = clear(self.lease.directory());
        }
    }
}
fn clear(path: &Path) -> Result<()> {
    for name in STATE {
        match std::fs::remove_file(path.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn read(path: &Path) -> Option<Identity> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(65537)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 65536 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
fn publish(path: &Path, name: &str, identity: &Identity) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path)?;
    super::security::set_file_owner(file.as_file())?;
    file.write_all(&serde_json::to_vec(identity)?)?;
    file.as_file().sync_all()?;
    file.persist(path.join(name))?;
    Ok(())
}
fn existing(path: &Path) -> Result<Option<Directory>> {
    anyhow::ensure!(path.is_absolute(), "runtime path must be absolute");
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(Some(Directory::create(path)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
pub fn status(path: &Path) -> Result<Option<Identity>> {
    let Some(directory) = existing(path)? else {
        return Ok(None);
    };
    let owner = runtime::owner_at(&directory);
    Ok(owner.filter(|identity| read(&path.join("ready.json")).as_ref() == Some(identity)))
}
pub fn stop(path: &Path, timeout: Duration) -> Result<()> {
    let Some(directory) = existing(path)? else {
        return Ok(());
    };
    let Some(owner) = runtime::owner_at(&directory) else {
        // Crash recovery requires exclusive ownership too; corrupt live owners
        // and concurrent startup cannot be mistaken for an abandoned directory.
        let lease = Lease::acquire(directory)?;
        return clear(lease.directory());
    };
    publish(path, "stop.json", &owner)?;
    crate::lifecycle::wait_until(timeout, || {
        Ok(runtime::owner_at(&directory).as_ref() != Some(&owner))
    })
}

struct Starting(Option<std::process::Child>);
impl Drop for Starting {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
/// Launch an independent interactive daemon. Only successful readiness transfers
/// ownership; errors or unwind retire the startup process. Daemon workers use jobs.
pub fn launch(program: &Path, path: &Path, timeout: Duration) -> Result<()> {
    use std::os::windows::process::CommandExt;
    if status(path)?.is_some() {
        return Ok(());
    }
    let directory = Directory::create(path)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.join("daemon.log"))?;
    super::security::set_file_owner(&log)?;
    let mut child = Starting(Some(
        std::process::Command::new(program)
            .arg("--runtime-dir")
            .arg(path)
            .arg("start")
            .creation_flags(0x08000000) // CREATE_NO_WINDOW; remain in this user's session.
            .stdin(std::process::Stdio::null())
            .stderr(log.try_clone()?)
            .stdout(log)
            .spawn()
            .context("start Windows daemon")?,
    ));
    crate::lifecycle::wait_until(timeout, || {
        // Another successful concurrent start is also the desired state.
        if status(directory.path())?.is_some() {
            return Ok(true);
        }
        anyhow::ensure!(
            child.0.as_mut().unwrap().try_wait()?.is_none(),
            "daemon exited before readiness; see daemon.log"
        );
        Ok(false)
    })?;
    let mut process = child.0.take().unwrap();
    std::thread::spawn(move || {
        let _ = process.wait();
    });
    Ok(())
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
