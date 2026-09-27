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

/// One in-flight USB preparation plus four bounded route retirements.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(120);

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
    pub fn publish_sessions(&self, sessions: &[crate::tablets::TabletSession]) -> Result<()> {
        anyhow::ensure!(self.owns_state(), "daemon state ownership changed");
        publish(
            self.lease.directory(),
            "sessions.json",
            &Snapshot {
                owner: self.lease.identity().clone(),
                sessions: sessions.to_vec(),
            },
        )
    }
    pub fn shutdown(self) -> Result<()> {
        anyhow::ensure!(self.owns_state(), "daemon state ownership changed");
        clear(self.lease.directory())
    }
    fn owns_state(&self) -> bool {
        runtime::owner_at(self.lease.private_directory()).as_ref() == Some(self.lease.identity())
    }
    pub fn stop_requested(&self) -> bool {
        read(&self.lease.directory().join("stop.json")).as_ref() == Some(self.lease.identity())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // Never retire replacement state. The stable lease stays held throughout.
        if self.owns_state() {
            let _ = clear(self.lease.directory());
        }
    }
}
fn clear(path: &Path) -> Result<()> {
    let mut result = Ok(());
    for name in STATE {
        match std::fs::remove_file(path.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                if result.is_ok() {
                    result = Err(error.into());
                }
            }
        }
    }
    result
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
fn publish(path: &Path, name: &str, identity: &impl serde::Serialize) -> Result<()> {
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
        Ok(_) => Ok(Some(Directory::open(path)?)),
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
        return reclaim(directory);
    };
    publish(path, "stop.json", &owner)?;
    crate::lifecycle::wait_until(timeout, || {
        Ok(runtime::owner_at(&directory).as_ref() != Some(&owner))
    })?;
    if runtime::owner_at(&directory).is_some() {
        return Ok(()); // A concurrent replacement owns its own state.
    }
    reclaim(directory)
}

fn reclaim(directory: Directory) -> Result<()> {
    let lease = Lease::acquire(directory)?;
    clear(lease.directory())
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

#[derive(serde::Serialize, serde::Deserialize)]
struct Snapshot {
    owner: Identity,
    sessions: Vec<crate::tablets::TabletSession>,
}
/// Read only the live daemon's bounded snapshot, never a previous process's state.
pub fn load_sessions(path: &Path) -> Option<Vec<crate::tablets::TabletSession>> {
    let owner = status(path).ok()??;
    let mut bytes = Vec::new();
    std::fs::File::open(path.join("sessions.json"))
        .ok()?
        .take(65537)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 65536 {
        return None;
    }
    let snapshot: Snapshot = serde_json::from_slice(&bytes).ok()?;
    (snapshot.owner == owner && status(path).ok().flatten().as_ref() == Some(&owner))
        .then_some(snapshot.sessions)
}
