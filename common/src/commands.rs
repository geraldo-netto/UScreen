//! Bounded subprocess operations shared by the daemon and settings GUI.
use std::io::{self, Read, Seek};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::Duration;

pub fn spawn_reaped(command: &mut std::process::Command) -> std::io::Result<u32> {
    let mut child = command.spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

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
    io::Error::new(io::ErrorKind::TimedOut, "command timed out")
}

impl SyncCommandExt for Command {
    fn output_timeout(&mut self, timeout: Duration) -> io::Result<Output> {
        let output = CapturedOutput::new()?;
        let mut child = self
            .stdin(Stdio::null())
            .stdout(output.stdout.try_clone()?)
            .stderr(output.stderr.try_clone()?)
            .spawn()?;
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return output.finish(status),
                Ok(None) if start.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
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
        let mut child = self
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(output.stdout.try_clone()?)
            .stderr(output.stderr.try_clone()?)
            .spawn()?;
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(status) => output.finish(status?),
            Err(_) => {
                // Await kill also waits for termination, so no zombie remains.
                let _ = child.kill().await;
                Err(timed_out())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
