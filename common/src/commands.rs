//! Command deadlines, not transactional cancellation of delegated operations.
//! Linux commands get a private process group. Timeout/cancellation signals its
//! members and reaps the direct child when permitted. Windows commands enter
//! an owned kill-on-close job before their suspended initial thread resumes.
//! Work delegated to another service can outlive the owned process tree.
use std::io::{self, Read, Seek};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::Duration;

#[cfg(windows)]
mod windows;

pub fn spawn_reaped(command: &mut std::process::Command) -> std::io::Result<u32> {
    let mut child = command.spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Grace allowed by the daemon's `stop` command.
pub const DAEMON_STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// Matches TimeoutStopSec in both shipped systemd user units (T268).
pub const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// A service stop may spend one budget on ExecStop and another retiring the
/// service process. Leave a further command budget for dispatch/restart.
pub fn daemon_command_timeout(managed: bool) -> Duration {
    if managed {
        SERVICE_STOP_TIMEOUT * 2 + COMMAND_TIMEOUT
    } else {
        DAEMON_STOP_TIMEOUT + COMMAND_TIMEOUT
    }
}

pub trait SyncCommandExt {
    fn output_bounded(&mut self) -> io::Result<Output> {
        self.output_timeout(COMMAND_TIMEOUT)
    }
    fn output_timeout(&mut self, timeout: Duration) -> io::Result<Output>;
}
// Anonymous files avoid pipe-capacity deadlocks and inherited pipe handles
// keeping output collection alive after the command itself has terminated.
struct CapturedOutput {
    stdout: std::fs::File,
    stderr: std::fs::File,
}
impl CapturedOutput {
    fn new() -> io::Result<Self> {
        Ok(Self {
            stdout: tempfile::tempfile()?,
            stderr: tempfile::tempfile()?,
        })
    }
    fn finish(mut self, status: ExitStatus) -> io::Result<Output> {
        self.stdout.rewind()?;
        self.stderr.rewind()?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        self.stdout.read_to_end(&mut stdout)?;
        self.stderr.read_to_end(&mut stderr)?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
}
fn timed_out() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "command timed out; detached or privileged work may still be running",
    )
}

fn prepare_group(command: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = command;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
}

fn terminate_group(pid: u32) {
    #[cfg(target_os = "linux")]
    {
        // The direct child is still owned and unreaped: its PID cannot have
        // been reused. prepare_group made that PID the new group's identity.
        let result = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
        if result < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!(
                pid,
                "Command group could not be terminated; delegated work may continue"
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = pid;
}

fn retire_sync(mut child: std::process::Child) {
    terminate_group(child.id());
    let signalled = child.kill();
    reap_after_signal(child, signalled);
}

fn reap_after_signal(mut child: std::process::Child, signalled: io::Result<()>) {
    if signalled.is_ok() {
        let _ = child.wait();
    } else {
        // A pkexec child may have changed UID. Waiting here would turn a
        // deadline into a wait for work we have no right to terminate.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

struct RunningAsync(tokio::process::Child);
impl Drop for RunningAsync {
    fn drop(&mut self) {
        // Tokio clears id() on wait/reap, avoiding signals to a recycled PID.
        // On cancellation signal the group before kill_on_drop retires child.
        if let Some(pid) = self.0.id() {
            terminate_group(pid);
        }
    }
}

impl SyncCommandExt for Command {
    fn output_timeout(&mut self, timeout: Duration) -> io::Result<Output> {
        let output = CapturedOutput::new()?;
        #[cfg(windows)]
        let job = windows::Job::new()?;
        prepare_group(self);
        let mut child = self
            .stdin(Stdio::null())
            .stdout(output.stdout.try_clone()?)
            .stderr(output.stderr.try_clone()?)
            .spawn()?;
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            if let Err(error) = job.start(child.id(), child.as_raw_handle()) {
                retire_sync(child);
                return Err(error);
            }
        }
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return output.finish(status),
                Ok(None) if start.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                result => {
                    retire_sync(child);
                    return Err(result.err().unwrap_or_else(timed_out));
                }
            }
        }
    }
}

pub trait AsyncCommandExt {
    fn output_bounded(&mut self) -> impl std::future::Future<Output = io::Result<Output>> + Send {
        self.output_timeout(COMMAND_TIMEOUT)
    }
    fn output_timeout(
        &mut self,
        timeout: Duration,
    ) -> impl std::future::Future<Output = io::Result<Output>> + Send;
}
impl AsyncCommandExt for tokio::process::Command {
    async fn output_timeout(&mut self, timeout: Duration) -> io::Result<Output> {
        let output = CapturedOutput::new()?;
        #[cfg(windows)]
        let job = windows::Job::new()?;
        prepare_group(self.as_std_mut());
        let mut child = RunningAsync(
            self.kill_on_drop(true)
                .stdin(Stdio::null())
                .stdout(output.stdout.try_clone()?)
                .stderr(output.stderr.try_clone()?)
                .spawn()?,
        );
        #[cfg(windows)]
        if let Err(error) = job.start(
            child.0.id().expect("new child"),
            child.0.raw_handle().expect("new child handle"),
        ) {
            let _ = child.0.kill().await;
            return Err(error);
        }
        match tokio::time::timeout(timeout, child.0.wait()).await {
            Ok(status) => output.finish(status?),
            Err(_) => {
                if let Some(pid) = child.0.id() {
                    terminate_group(pid);
                }
                // Await kill reaps when permitted. Permission failure returns
                // immediately; Tokio then owns eventual direct-child reaping.
                let _ = child.0.kill().await;
                Err(timed_out())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t328_denied_signal_does_not_extend_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let done = dir.path().join("done");
        let child = Command::new("sh")
            .args(["-c", "sleep 0.4; printf done > \"$1\"", "sh"])
            .arg(&done)
            .spawn()
            .unwrap();
        let pid = child.id();
        let start = std::time::Instant::now();
        // Isolate the EPERM result of a privileged child without authorizing
        // real system setup or depending on root in the normal test suite.
        reap_after_signal(child, Err(io::Error::from(io::ErrorKind::PermissionDenied)));
        let elapsed = start.elapsed();
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            done.exists(),
            "T328: denied work can continue after the deadline"
        );
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        assert!(
            elapsed < Duration::from_millis(200),
            "T328: denied cancellation blocked for {elapsed:?}"
        );
        assert!(timed_out().to_string().contains("may still be running"));
    }

    #[test]
    fn t268_lifecycle_deadlines_cover_cli_and_shipped_service_budgets() {
        assert!(daemon_command_timeout(false) > DAEMON_STOP_TIMEOUT);
        assert!(SERVICE_STOP_TIMEOUT > DAEMON_STOP_TIMEOUT);
        assert!(daemon_command_timeout(true) > SERVICE_STOP_TIMEOUT * 2);
        for unit in [
            include_str!("../../scripts/uscreen.service"),
            include_str!("../../packaging/uscreen.service"),
        ] {
            let seconds: u64 = unit
                .lines()
                .find_map(|line| line.strip_prefix("TimeoutStopSec="))
                .expect("T268: explicit service stop deadline")
                .parse()
                .unwrap();
            assert_eq!(Duration::from_secs(seconds), SERVICE_STOP_TIMEOUT);
        }
    }

    fn assert_reaped(path: &std::path::Path) {
        let pid = std::fs::read_to_string(path).unwrap();
        assert!(!std::path::Path::new(&format!("/proc/{}", pid.trim())).exists());
    }

    // T094: subprocess deadlines must kill and reap, not abandon hung commands.
    #[test]
    fn t094_sync_command_is_bounded_and_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pid");
        let start = std::time::Instant::now();
        let result = Command::new("sh")
            .args(["-c", "echo $$ > \"$1\"; exec sleep 1", "sh"])
            .arg(&path)
            .output_timeout(Duration::from_millis(50));
        assert!(result.is_err_and(|e| e.kind() == io::ErrorKind::TimedOut));
        assert!(start.elapsed() < Duration::from_millis(800));
        assert_reaped(&path);
    }

    #[tokio::test]
    async fn t094_async_command_is_bounded_and_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pid");
        let result = tokio::process::Command::new("sh")
            .args(["-c", "echo $$ > \"$1\"; exec sleep 1", "sh"])
            .arg(&path)
            .output_timeout(Duration::from_millis(50))
            .await;
        assert!(result.is_err_and(|e| e.kind() == io::ErrorKind::TimedOut));
        assert_reaped(&path);
    }

    #[tokio::test]
    async fn t094_cancelled_command_is_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pid");
        let task_path = path.clone();
        let task = tokio::spawn(async move {
            tokio::process::Command::new("sh")
                .args(["-c", "echo $$ > \"$1\"; exec sleep 1", "sh"])
                .arg(task_path)
                .output_bounded()
                .await
        });
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_reaped(&path);
    }
}
