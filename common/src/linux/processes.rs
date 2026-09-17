//! Native process snapshots and identity-bound signalling for Linux adapters.
use anyhow::{Context, Result};
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureRole {
    Helper,
    Encoder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub uid: u32,
    pub start_ticks: u64,
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub cwd: PathBuf,
}

pub fn start_ticks(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

impl Process {
    pub fn read(pid: u32) -> Option<Self> {
        let root = PathBuf::from(format!("/proc/{pid}"));
        let start = start_ticks(pid)?;
        let process = Self {
            pid,
            uid: std::fs::metadata(&root).ok()?.uid(),
            start_ticks: start,
            executable: std::fs::read_link(root.join("exe")).ok()?,
            arguments: std::fs::read(root.join("cmdline"))
                .ok()?
                .split(|&b| b == 0)
                .map(|argument| OsString::from_vec(argument.to_vec()))
                .collect(),
            cwd: std::fs::read_link(root.join("cwd")).ok()?,
        };
        (start_ticks(pid) == Some(start)).then_some(process)
    }

    pub fn owned_by(&self, uid: u32) -> bool {
        self.uid == uid
    }

    pub fn executable_named(&self, name: &str) -> bool {
        let Some(file) = self.executable.file_name() else {
            return false;
        };
        let bytes = file.as_bytes();
        // /proc marks an old inode after an atomic binary upgrade.
        bytes.strip_suffix(b" (deleted)").unwrap_or(bytes) == name.as_bytes()
    }

    pub fn capture_role(&self, fifo: &Path) -> Option<CaptureRole> {
        if self.executable_named("evdi_helper") && self.has_path_argument("--capture-fifo", fifo) {
            Some(CaptureRole::Helper)
        } else if self.executable_named("ffmpeg") && self.has_path_argument("-i", fifo) {
            Some(CaptureRole::Encoder)
        } else {
            None
        }
    }

    pub fn has_path_argument(&self, option: &str, path: &Path) -> bool {
        let Ok(path) = std::path::absolute(path) else {
            return false;
        };
        self.arguments
            .windows(2)
            .any(|pair| pair[0] == OsStr::new(option) && self.cwd.join(&pair[1]) == path)
    }
}

trait Inventory {
    fn pids(&self) -> io::Result<Vec<u32>>;
    fn uid(&self, pid: u32) -> Option<u32>;
    fn name(&self, pid: u32) -> Option<String>;
    fn read(&self, pid: u32) -> Option<Process>;
}

struct ProcInventory;
impl Inventory for ProcInventory {
    fn pids(&self) -> io::Result<Vec<u32>> {
        Ok(std::fs::read_dir("/proc")?
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .collect())
    }
    fn uid(&self, pid: u32) -> Option<u32> {
        Some(std::fs::metadata(format!("/proc/{pid}")).ok()?.uid())
    }
    fn name(&self, pid: u32) -> Option<String> {
        Some(
            std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .ok()?
                .trim()
                .into(),
        )
    }
    fn read(&self, pid: u32) -> Option<Process> {
        Process::read(pid)
    }
}

fn inventory(source: &impl Inventory, uid: u32, name: Option<&str>) -> io::Result<Vec<Process>> {
    Ok(source
        .pids()?
        .into_iter()
        .filter(|&pid| source.uid(pid) == Some(uid))
        .filter(|&pid| name.is_none_or(|name| source.name(pid).as_deref() == Some(name)))
        .filter_map(|pid| source.read(pid))
        // A PID can be replaced between the cheap filter and full snapshot.
        .filter(|process| process.owned_by(uid))
        .collect())
}

pub fn same_user_processes() -> io::Result<Vec<Process>> {
    inventory(&ProcInventory, unsafe { libc::getuid() }, None)
}

/// Read-only candidate filter; callers still validate the complete identity.
pub fn same_user_processes_named(name: &str) -> io::Result<Vec<Process>> {
    inventory(&ProcInventory, unsafe { libc::getuid() }, Some(name))
}

struct ProcessHandle(OwnedFd);
impl ProcessHandle {
    fn pin(process: &Process) -> Result<Option<Self>> {
        anyhow::ensure!(
            process.owned_by(unsafe { libc::getuid() }),
            "process belongs to another user"
        );
        // pidfd_open exists since Linux 5.3. Do not fall back to a PID that can
        // be reused between validation and signalling; fail before capture.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, process.pid, 0) };
        if fd < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(None);
            }
            return Err(error).context("pin capture process with pidfd_open");
        }
        let handle = Self(unsafe { OwnedFd::from_raw_fd(fd as i32) });
        Ok((Process::read(process.pid).as_ref() == Some(process)).then_some(handle))
    }

    fn signal(&self, signal: i32) -> io::Result<()> {
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.0.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if result >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }

    fn exited(&self) -> io::Result<bool> {
        let mut poll = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(poll.revents & (libc::POLLIN | libc::POLLHUP) != 0)
    }
}

async fn await_exit(handles: &[ProcessHandle], budget: Duration) -> io::Result<bool> {
    let wait = async {
        loop {
            let states = handles
                .iter()
                .map(ProcessHandle::exited)
                .collect::<io::Result<Vec<_>>>()?;
            if states.into_iter().all(|exited| exited) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    match tokio::time::timeout(budget, wait).await {
        Ok(result) => result.map(|()| true),
        Err(_) => Ok(false),
    }
}

/// Retire all selected processes within one shared grace and kill budget.
/// Handles remain tied to the original processes even when their PIDs are reused.
pub async fn retire(
    processes: &[Process],
    grace: Duration,
    kill_budget: Duration,
) -> Result<usize> {
    let handles = processes
        .iter()
        .map(ProcessHandle::pin)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    for handle in &handles {
        handle.signal(libc::SIGTERM)?;
    }
    if !await_exit(&handles, grace).await? {
        for handle in &handles {
            handle.signal(libc::SIGKILL)?;
        }
        anyhow::ensure!(
            await_exit(&handles, kill_budget).await?,
            "capture processes did not retire within deadline"
        );
    }
    Ok(handles.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeInventory {
        reads: Cell<usize>,
        names: Cell<usize>,
    }
    impl Inventory for FakeInventory {
        fn pids(&self) -> io::Result<Vec<u32>> {
            Ok((1..=1000).collect())
        }
        fn uid(&self, pid: u32) -> Option<u32> {
            Some(if pid <= 100 { 1000 } else { 2000 })
        }
        fn name(&self, pid: u32) -> Option<String> {
            self.names.set(self.names.get() + 1);
            Some(if pid <= 2 { "uscreen" } else { "other" }.into())
        }
        fn read(&self, pid: u32) -> Option<Process> {
            self.reads.set(self.reads.get() + 1);
            Some(Process {
                pid,
                uid: if pid == 2 { 2000 } else { 1000 },
                start_ticks: 1,
                executable: PathBuf::from(OsString::from_vec(b"/native-\xff/uscreen".to_vec())),
                arguments: vec!["uscreen".into()],
                cwd: "/".into(),
            })
        }
    }

    #[test]
    fn t409_inventory_filters_before_full_reads_and_rechecks_uid() {
        let source = FakeInventory {
            reads: Cell::new(0),
            names: Cell::new(0),
        };
        let selected = inventory(&source, 1000, Some("uscreen")).unwrap();
        assert_eq!(source.names.get(), 100, "other users need no comm read");
        assert_eq!(
            source.reads.get(),
            2,
            "only same-user named candidates need full snapshots"
        );
        assert_eq!(
            selected.len(),
            1,
            "UID change after prefilter must be rejected"
        );
        assert_eq!(selected[0].pid, 1);
        assert_eq!(
            selected[0].executable.as_os_str().as_bytes(),
            b"/native-\xff/uscreen"
        );
        source.reads.set(0);
        let all = inventory(&source, 1000, None).unwrap();
        assert_eq!(source.reads.get(), 100);
        assert_eq!(
            all.len(),
            99,
            "full discovery must retain all same-user names"
        );
    }

    #[test]
    fn t245_process_matching_preserves_ownership_and_native_arguments() {
        let mut process = Process {
            pid: 42,
            uid: 1000,
            start_ticks: 1,
            executable: "/bin/ffmpeg (deleted)".into(),
            arguments: vec![
                "ffmpeg".into(),
                "-i".into(),
                OsString::from_vec(b"/tmp/native-\xff/capture.fifo".to_vec()),
            ],
            cwd: "/".into(),
        };
        let path = PathBuf::from(process.arguments[2].clone());
        assert!(process.owned_by(1000));
        assert!(!process.owned_by(1001));
        assert!(process.executable_named("ffmpeg"));
        assert!(!process.executable_named("evdi_helper"));
        assert!(process.has_path_argument("-i", &path));
        assert!(!process.has_path_argument("-i", &PathBuf::from(path.to_string_lossy().as_ref())));
        process.arguments[1] = "-metadata".into();
        assert!(!process.has_path_argument("-i", &path));
    }
}
