//! T529: packed NV12 into stock FFmpeg over owned native stdin/stdout pipes.
//! Caller drains output concurrently using the existing framed packet contract.
//! Hardware discovery and capture/ADB integration remain separate backends.
use crate::{
    ffmpeg_args::Settings,
    raw_transfer::{FrameWriter, MAX_WRITE_TIMEOUT},
};
use anyhow::{ensure, Context, Result};
use blent_config::commands::OwnedChild;
use std::{
    ffi::OsStr,
    future::Future,
    io,
    pin::Pin,
    process::{ExitStatus, Stdio},
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tokio::{
    io::AsyncWrite,
    process::{ChildStdin, ChildStdout, Command},
};

pub struct Encoder {
    pub input: FrameWriter<OwnedInput>,
    pub output: ChildStdout,
}

impl Encoder {
    /// Start the software fallback. Startup proves process creation, not codec
    /// readiness: the consumer must validate the first framed encoded packet.
    /// Resizing requires retiring this session and starting a fresh one.
    pub fn start(
        program: &OsStr,
        settings: &Settings,
        dimensions: (u32, u32),
        timeout: Duration,
    ) -> Result<Self> {
        validate(settings, dimensions, timeout)?;
        let args = settings.arguments(OsStr::new("pipe:0"), false, dimensions.0, dimensions.1)?;
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        Self::spawn(&mut command, dimensions, timeout)
    }

    fn spawn(command: &mut Command, dimensions: (u32, u32), timeout: Duration) -> Result<Self> {
        let mut child = OwnedChild::spawn(command).context("start pipe encoder")?;
        let stdin = child.take_stdin().context("encoder input pipe missing")?;
        let output = child.take_stdout().context("encoder output pipe missing")?;
        let input = OwnedInput {
            stdin: Some(stdin),
            child: Some(child),
            finishing: None,
            timeout,
        };
        Ok(Self {
            input: FrameWriter::new(input, dimensions, timeout)?,
            output,
        })
    }
}

fn validate(settings: &Settings, dimensions: (u32, u32), timeout: Duration) -> Result<()> {
    ensure!(
        settings.encoder == "libx264",
        "pipe encoder currently supports the libx264 software fallback only"
    );
    blent_config::raw_frame::Layout::new(dimensions.0, dimensions.1, 2)?;
    ensure!(
        !timeout.is_zero() && timeout <= MAX_WRITE_TIMEOUT,
        "invalid pipe encoder deadline"
    );
    ensure!(
        (1..=blent_config::MAX_FPS).contains(&settings.fps),
        "invalid pipe encoder frame rate"
    );
    ensure!(
        (blent_config::MIN_BITRATE_KBPS..=blent_config::MAX_BITRATE_KBPS)
            .contains(&settings.bitrate),
        "invalid pipe encoder bitrate"
    );
    ensure!(
        (blent_config::MIN_QUALITY..=blent_config::MAX_QUALITY).contains(&settings.quality),
        "invalid pipe encoder quality"
    );
    settings.profile()?;
    Ok(())
}

type Completion = Pin<Box<dyn Future<Output = io::Result<ExitStatus>> + Send>>;

/// Ownership travels with the write operation. A partial write that fails,
/// times out, or is cancelled drops both stdin and the process/job owner.
pub struct OwnedInput {
    stdin: Option<ChildStdin>,
    child: Option<OwnedChild>,
    finishing: Option<Completion>,
    timeout: Duration,
}

impl AsyncWrite for OwnedInput {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        };
        Pin::new(stdin).poll_write(cx, bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        Pin::new(stdin).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        // T529: Windows stdio accepts bytes into a blocking worker first.
        // Its shutdown is a no-op; flush observes the pending write result.
        if let Some(stdin) = self.stdin.as_mut() {
            std::task::ready!(Pin::new(stdin).poll_flush(cx))?;
        }
        // Closing the real handle sends EOF on every platform. Keep the owner
        // alive while FFmpeg flushes its final packet and exits.
        drop(self.stdin.take());
        if let Some(child) = self.child.take() {
            self.finishing = Some(Box::pin(child.finish(self.timeout)));
        }
        let Some(finishing) = self.finishing.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        let status = std::task::ready!(finishing.as_mut().poll(cx));
        self.finishing = None;
        Poll::Ready(status.and_then(success))
    }
}

fn success(status: ExitStatus) -> io::Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("pipe encoder exited: {status}")))
    }
}

#[cfg(test)]
mod tests;
