//! Per-user runtime state: the capture FIFO and the session token.
//!
//! Both used to live in /tmp, world-readable. On a multi-user machine that
//! meant any local account could open the FIFO and read the raw frames — a
//! live copy of the screen — or write into it and corrupt the stream. The
//! directory is intended to be private. Creation requests mode 0700, but
//! existing paths and creation errors are not validated yet (T252).

use anyhow::{Context, Result};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

/// Use an existing XDG_RUNTIME_DIR, then /run/user/<uid>, then HOME/.cache
/// (/tmp/.cache if HOME is absent), with a uscreen subdirectory. Creation
/// requests 0700; existing-directory validation remains unresolved (T252).
pub fn runtime_dir() -> PathBuf {
    // /run/user/<uid> next: the daemon under systemd and a `doctor` run from
    // an environment-scrubbed shell (sudo, cron) must agree on the path, or
    // the orphan check looks for a FIFO that is somewhere else.
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| {
            let p = PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() }));
            p.is_dir().then_some(p)
        })
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".cache")
        });
    let dir = base.join("uscreen");
    if !dir.is_dir() {
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir);
    }
    dir
}

/// One FIFO per virtual display; the first keeps the old name.
pub fn fifo_path_for(instance: u32) -> PathBuf {
    if instance == 0 {
        runtime_dir().join("capture.fifo")
    } else {
        runtime_dir().join(format!("capture-{}.fifo", instance))
    }
}

fn token_path() -> PathBuf {
    runtime_dir().join("token")
}

/// 64 hex characters from the kernel's RNG. Generated once per daemon run and
/// written to the runtime directory (0600) for anything else of ours that
/// needs it; it is never sent anywhere except to the tablet, over adb.
pub fn new_session_token() -> Result<String> {
    use std::io::Read;
    let mut raw = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .context("open /dev/urandom")?
        .read_exact(&mut raw)
        .context("read /dev/urandom")?;
    let token: String = raw.iter().map(|b| format!("{:02x}", b)).collect();

    let path = token_path();
    let _ = std::fs::remove_file(&path);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("write {}", path.display()))?;
    use std::io::Write;
    f.write_all(token.as_bytes())?;
    Ok(token)
}

/// Compare a presented token with the expected one. Constant-time over the
/// expected length, so timing does not leak how many leading characters were
/// right — cheap insurance on a loopback socket.
pub fn token_matches(expected: &str, presented: &str) -> bool {
    let a = expected.as_bytes();
    let b = presented.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn t099_session_snapshot_is_private_current_and_removed_on_exit() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("sessions.json");
        let session = TabletSession {
            serial: "TABLET".into(),
            instance: 1,
            video_port: 19002,
            input_port: 19102,
        };
        {
            let mut ledger = SessionLedger::new(path.clone()).unwrap();
            ledger.update(vec![session.clone()]).unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let snapshot: SessionSnapshot =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(snapshot.sessions.as_slice(), std::slice::from_ref(&session));
        }
        assert!(!path.exists());
        // A real named process exercises liveness and PID-start-time validation.
        let executable = temp.path().join("uscreen");
        std::os::unix::fs::symlink("/bin/sleep", &executable).unwrap();
        let mut child = tokio::process::Command::new(executable)
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let mut snapshot = SessionSnapshot {
            pid,
            start_ticks: process_start(pid).unwrap(),
            sessions: vec![session.clone()],
        };
        std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert_eq!(load_sessions(&path), Some(vec![session]));
        snapshot.start_ticks += 1;
        std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert!(load_sessions(&path).is_none());
        snapshot.start_ticks -= 1;
        std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        assert!(load_sessions(&path).is_none());
    }

    #[test]
    fn token_comparison_is_exact() {
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc123", "abc124"));
        assert!(!token_matches("abc123", "abc12"));
        assert!(!token_matches("abc123", ""));
    }

    #[test]
    fn runtime_dir_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = runtime_dir();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        // Either we created it 0700, or it is the user's own cache dir.
        assert_eq!(
            mode & 0o077,
            0,
            "runtime dir {} is group/world accessible",
            dir.display()
        );
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TabletSession {
    pub serial: String,
    pub instance: u32,
    pub video_port: u16,
    pub input_port: u16,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SessionSnapshot {
    pid: u32,
    start_ticks: u64,
    sessions: Vec<TabletSession>,
}

fn process_start(pid: u32) -> Option<u64> {
    super::processes::start_ticks(pid)
}

pub fn load_sessions(path: &std::path::Path) -> Option<Vec<TabletSession>> {
    let snapshot: SessionSnapshot = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (crate::daemon_is_running(snapshot.pid)
        && process_start(snapshot.pid) == Some(snapshot.start_ticks))
    .then_some(snapshot.sessions)
}

/// Atomic private snapshot of the monitor's actual assignments, including CLI
/// port overrides. PID plus start time prevents accepting a reused process ID.
pub struct SessionLedger {
    path: PathBuf,
    snapshot: SessionSnapshot,
}
impl SessionLedger {
    pub fn new(path: PathBuf) -> Result<Self> {
        let pid = std::process::id();
        let ledger = Self {
            path,
            snapshot: SessionSnapshot {
                pid,
                start_ticks: process_start(pid).context("read daemon start time")?,
                sessions: Vec::new(),
            },
        };
        ledger.write()?;
        Ok(ledger)
    }

    fn write(&self) -> Result<()> {
        let mut temporary =
            tempfile::NamedTempFile::new_in(self.path.parent().context("session directory")?)?;
        serde_json::to_writer(&mut temporary, &self.snapshot)?;
        temporary.persist(&self.path)?;
        Ok(())
    }

    pub fn update(&mut self, mut sessions: Vec<TabletSession>) -> Result<()> {
        sessions.sort_by_key(|session| session.instance);
        if self.snapshot.sessions != sessions {
            let previous = std::mem::replace(&mut self.snapshot.sessions, sessions);
            if let Err(error) = self.write() {
                self.snapshot.sessions = previous;
                return Err(error);
            }
        }
        Ok(())
    }
}
impl Drop for SessionLedger {
    fn drop(&mut self) {
        if let Ok(data) = std::fs::read(&self.path) {
            if let Ok(snapshot) = serde_json::from_slice::<SessionSnapshot>(&data) {
                if snapshot.pid == self.snapshot.pid
                    && snapshot.start_ticks == self.snapshot.start_ticks
                {
                    let _ = std::fs::remove_file(&self.path);
                }
            }
        }
    }
}
