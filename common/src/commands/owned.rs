//! Owned streaming children. Native process/job APIs stay in command adapters.
use super::{prepare_group, timed_out, RunningAsync};
use std::{io, process::ExitStatus, time::Duration};
use tokio::process::{ChildStdin, ChildStdout, Command};

pub struct OwnedChild {
    pub(super) inner: RunningAsync,
    // The job outlives the running child and every borrowed wait operation.
    #[cfg(windows)]
    _job: super::windows::Job,
}

impl OwnedChild {
    /// Preserve caller-selected stdio. On Windows, no child thread runs before
    /// job assignment. Drop/cancellation retires the owned process tree.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        #[cfg(windows)]
        let job = super::windows::Job::new()?;
        prepare_group(command.as_std_mut());
        let inner = RunningAsync(command.kill_on_drop(true).spawn()?);
        #[cfg(windows)]
        job.start(
            inner.0.id().expect("new child"),
            inner.0.raw_handle().expect("new child handle"),
        )?;
        Ok(Self {
            inner,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub fn id(&self) -> Option<u32> {
        self.inner.0.id()
    }

    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.inner.0.stdin.take()
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.inner.0.stdout.take()
    }

    /// Consume ownership. Expiry or cancellation drops the native owner and
    /// lets Tokio reap the direct child; callers cannot reuse a retired child.
    pub async fn finish(mut self, timeout: Duration) -> io::Result<ExitStatus> {
        tokio::time::timeout(timeout, self.inner.0.wait())
            .await
            .map_err(|_| timed_out())?
    }
}
