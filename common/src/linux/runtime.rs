//! Per-user runtime state: the capture FIFO and the session token.
//!
//! Both used to live in /tmp, world-readable. On a multi-user machine that
//! meant any local account could open the FIFO and read the raw frames — a
//! live copy of the screen — or write into it and corrupt the stream. The
//! directory must be owned by this user and private. Unsafe existing paths
//! and creation errors are rejected before a token or FIFO path is returned.

use anyhow::{Context, Result};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

/// Select the documented base, then require owned, usable directories. A
/// selected unsafe base/path is an error, never a silent switch to another path.
pub fn runtime_dir() -> Result<PathBuf> {
    let uid = unsafe { libc::getuid() };
    let base = select_runtime_base(
        std::env::var_os("XDG_RUNTIME_DIR"),
        PathBuf::from(format!("/run/user/{uid}")),
        std::env::var_os("HOME"),
    );
    create_runtime_directory(&base)?;
    // HOME/.cache may intentionally be a symlink. Resolve the base once and
    // validate its actual directory; the final uscreen component must not be a link.
    let base = std::fs::canonicalize(&base).context("resolve runtime base")?;
    validate_runtime_directory(&base, uid, false)?;
    let dir = base.join("uscreen");
    create_runtime_directory(&dir)?;
    validate_runtime_directory(&dir, uid, true)?;
    Ok(dir)
}

fn select_runtime_base(
    xdg: Option<std::ffi::OsString>,
    user_runtime: PathBuf,
    home: Option<std::ffi::OsString>,
) -> PathBuf {
    xdg.map(PathBuf::from)
        .filter(|path| path.is_dir())
        .or_else(|| user_runtime.is_dir().then_some(user_runtime))
        .unwrap_or_else(|| PathBuf::from(home.unwrap_or_else(|| "/tmp".into())).join(".cache"))
}

fn create_runtime_directory(path: &std::path::Path) -> Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .with_context(|| format!("create runtime directory {}", path.display()))
}

fn validate_runtime_directory(path: &std::path::Path, uid: u32, private: bool) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| {
            format!(
                "open runtime directory without following links: {}",
                path.display()
            )
        })?;
    let metadata = directory.metadata()?;
    anyhow::ensure!(
        metadata.uid() == uid,
        "runtime directory {} belongs to UID {}, expected {uid}",
        path.display(),
        metadata.uid()
    );
    let forbidden = if private { 0o077 } else { 0o022 };
    anyhow::ensure!(
        metadata.mode() & forbidden == 0 && metadata.mode() & 0o700 == 0o700,
        "unsafe runtime directory permissions {:o}: {}",
        metadata.mode() & 0o777,
        path.display()
    );
    Ok(())
}

/// One FIFO per virtual display; the first keeps the old name.
pub fn fifo_path_for(instance: u32) -> Result<PathBuf> {
    let name = if instance == 0 {
        "capture.fifo".into()
    } else {
        format!("capture-{instance}.fifo")
    };
    Ok(runtime_dir()?.join(name))
}

fn token_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("token"))
}

/// 64 hex characters from the kernel RNG, without publishing or logging them.
pub fn random_token() -> Result<String> {
    use std::io::Read;
    let mut raw = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .context("open /dev/urandom")?
        .read_exact(&mut raw)
        .context("read /dev/urandom")?;
    let token: String = raw.iter().map(|b| format!("{:02x}", b)).collect();

    Ok(token)
}

/// Create the initial private credential file; attachments rotate it thereafter.
pub fn new_session_token() -> Result<String> {
    let token = random_token()?;
    let path = token_path()?;
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
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn t444_random_tokens_are_fresh_fixed_width_and_reject_mutations() {
        let first = random_token().unwrap();
        let second = random_token().unwrap();
        assert_ne!(first, second);
        for token in [&first, &second] {
            assert_eq!(token.len(), 64);
            assert!(token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
            for index in 0..token.len() {
                let mut bytes = token.as_bytes().to_vec();
                bytes[index] = b'x';
                let mutated = String::from_utf8(bytes).unwrap();
                assert!(!token_matches(token, &mutated));
            }
            for length in [0, 1, 63, 65, 4096] {
                assert!(!token_matches(token, &"a".repeat(length)));
            }
        }
    }

    #[test]
    fn t436_runtime_fixture_survives_shared_umask() {
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", "umask 0002; exec \"$@\"", "uscreen-t436"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux::runtime::tests::t252_runtime_paths_reject_unsafe_state",
                "--nocapture",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T436: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // T252: child processes isolate runtime environment and use only disposable
    // directories. Existing private files must never be created through unsafe paths.
    #[test]
    fn t252_runtime_paths_reject_unsafe_state() {
        for scenario in [
            "new",
            "safe",
            "0755",
            "0777",
            "symlink",
            "file",
            "unwritable",
        ] {
            let root = tempfile::Builder::new()
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "linux::runtime::tests::t252_runtime_child",
                    "--nocapture",
                ])
                .env("USCREEN_T252_CASE", scenario)
                .env("XDG_RUNTIME_DIR", root.path())
                .env("HOME", root.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "T252 {scenario}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn t252_runtime_child() {
        use std::os::unix::fs::PermissionsExt;
        let Ok(scenario) = std::env::var("USCREEN_T252_CASE") else {
            return;
        };
        let base = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        let dir = base.join("uscreen");
        match scenario.as_str() {
            "safe" | "0755" | "0777" => {
                std::fs::create_dir(&dir).unwrap();
                let mode = match scenario.as_str() {
                    "0755" => 0o755,
                    "0777" => 0o777,
                    _ => 0o700,
                };
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
            }
            "symlink" => {
                let target = base.join("target");
                std::fs::create_dir(&target).unwrap();
                std::os::unix::fs::symlink(target, &dir).unwrap();
            }
            "file" => std::fs::write(&dir, "not a directory").unwrap(),
            "unwritable" => {
                std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o500)).unwrap()
            }
            _ => {}
        }
        let result = new_session_token();
        let accepted = matches!(scenario.as_str(), "new" | "safe");
        assert_eq!(
            result.is_ok(),
            accepted,
            "T252 {scenario}: {:?}",
            result.as_ref().err()
        );
        assert_eq!(
            fifo_path_for(0).is_ok(),
            accepted,
            "T252 FIFO path authorization"
        );
        if accepted {
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(dir.join("token"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        } else {
            assert!(!dir.join("token").exists());
            assert!(!base.join("target/token").exists());
        }
    }

    #[test]
    fn t252_directory_owner_must_match_the_calling_user() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let uid = unsafe { libc::getuid() };
        let error = validate_runtime_directory(root.path(), uid.wrapping_add(1), true).unwrap_err();
        assert!(error.to_string().contains("belongs to UID"), "{error:#}");
        validate_runtime_directory(root.path(), uid, true).unwrap();
    }

    #[test]
    fn t252_foreign_owner_child() {
        let Some(root) = std::env::var_os("USCREEN_T252_OWNER_ROOT") else {
            return;
        };
        use std::os::unix::ffi::OsStrExt;
        let dir = PathBuf::from(root).join("uscreen");
        create_runtime_directory(&dir).unwrap();
        let uid = unsafe { libc::getuid() };
        if uid == 0 {
            // The isolated CI container can create a genuinely foreign-owned
            // fixture. No real user/runtime directory is touched.
            let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::chown(path.as_ptr(), 1, 1) }, 0);
            let error = new_session_token().unwrap_err();
            assert!(error.to_string().contains("belongs to UID"), "{error:#}");
            assert!(fifo_path_for(0).is_err());
        } else {
            // Unprivileged local runs exercise the same owner validator with
            // a different caller UID; actual chown is covered in the container.
            assert!(validate_runtime_directory(&dir, uid.wrapping_add(1), true).is_err());
        }
        assert!(!dir.join("token").exists());
    }

    #[test]
    fn t252_foreign_owner_cannot_receive_runtime_files() {
        let root = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux::runtime::tests::t252_foreign_owner_child",
                "--nocapture",
            ])
            .env("USCREEN_T252_OWNER_ROOT", root.path())
            .env("XDG_RUNTIME_DIR", root.path())
            .env("HOME", root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T252 foreign owner: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn t252_fallback_selection_preserves_native_home_and_order() {
        use std::os::unix::ffi::OsStringExt;
        let root = tempfile::tempdir().unwrap();
        let xdg = root.path().join("xdg");
        let user = root.path().join("user");
        let home = root
            .path()
            .join(std::ffi::OsString::from_vec(b"home-\xff".to_vec()));
        let choose = |xdg: Option<&std::path::Path>| {
            select_runtime_base(
                xdg.map(|path| path.as_os_str().to_owned()),
                user.clone(),
                Some(home.clone().into_os_string()),
            )
        };
        assert_eq!(choose(None), home.join(".cache"));
        assert_eq!(choose(Some(&xdg)), home.join(".cache"));
        std::fs::create_dir(&user).unwrap();
        assert_eq!(choose(Some(&xdg)), user);
        std::fs::create_dir(&xdg).unwrap();
        assert_eq!(choose(Some(&xdg)), xdg);
        assert_eq!(
            select_runtime_base(None, root.path().join("absent"), None),
            PathBuf::from("/tmp/.cache")
        );
    }

    // T242: the fake tablet consumes the same base-selection fixture.
    #[test]
    fn t242_fake_tablet_runtime_fixture_matches_the_host() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../../testdata/runtime-bases.json")).unwrap();
        for case in cases {
            let root = tempfile::tempdir().unwrap();
            for directory in case["directories"].as_array().unwrap() {
                std::fs::create_dir(root.path().join(directory.as_str().unwrap())).unwrap();
            }
            let actual = select_runtime_base(
                t242_fixture_path(root.path(), &case["xdg"]),
                root.path().join("run"),
                t242_fixture_path(root.path(), &case["home"]),
            );
            let expected = t242_fixture_path(root.path(), &case["expected"]).unwrap();
            assert_eq!(actual, PathBuf::from(expected), "T242 {}", case["name"]);
        }
    }

    fn t242_fixture_path(
        root: &std::path::Path,
        value: &serde_json::Value,
    ) -> Option<std::ffi::OsString> {
        value.as_str().map(|path| match path {
            "" | ".cache" => path.into(),
            _ => root.join(path).into_os_string(),
        })
    }

    #[test]
    fn t252_base_permissions_and_symlink_policy_are_explicit() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let uid = unsafe { libc::getuid() };
        for mode in [0o700, 0o755, 0o775, 0o777, 0o500] {
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(
                validate_runtime_directory(root.path(), uid, false).is_ok(),
                matches!(mode, 0o700 | 0o755),
                "T252 base mode {mode:o}"
            );
        }
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(root.path(), &link).unwrap();
        assert!(validate_runtime_directory(&link, uid, true).is_err());
        validate_runtime_directory(&std::fs::canonicalize(&link).unwrap(), uid, false).unwrap();
    }

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
        let dir = runtime_dir().unwrap();
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
