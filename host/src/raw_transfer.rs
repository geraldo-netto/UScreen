//! T635: bounded packed-NV12 writes shared by native encoder pipe adapters.
//!
//! A raw stream has no frame delimiter. Failed or cancelled writes therefore
//! drop the owned writer; the next frame requires a new encoder session. Native
//! adapters own process creation/retirement and supply an exclusively owned
//! AsyncWrite handle whose drop closes input, such as ChildStdin.
use std::{io, time::Duration};
use tokio::io::{AsyncWrite, AsyncWriteExt};

/// Default tolerates brief encoder stalls; callers can choose a shorter budget.
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_WRITE_TIMEOUT: Duration = Duration::from_secs(60);

pub struct FrameWriter<W> {
    writer: Option<W>,
    dimensions: (u32, u32),
    bytes: usize,
    timeout: Duration,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    /// Canonical NV12 geometry follows the shared raw-frame layout contract.
    /// A nonzero deadline up to 60 seconds bounds backpressure. No frame queue
    /// or extra frame allocation is introduced; the caller retains its buffer.
    pub fn new(writer: W, dimensions: (u32, u32), timeout: Duration) -> io::Result<Self> {
        let layout = blent_config::raw_frame::Layout::new(dimensions.0, dimensions.1, 2)
            .map_err(|error| invalid(error.to_string()))?;
        if timeout.is_zero() || timeout > MAX_WRITE_TIMEOUT {
            return Err(invalid(
                "raw-frame write deadline must be nonzero and at most 60 seconds",
            ));
        }
        Ok(Self {
            writer: Some(writer),
            dimensions,
            bytes: layout.frame_bytes(),
            timeout,
        })
    }

    /// Write exactly one packed frame. Resize and malformed input are rejected
    /// before touching the stream, so valid writes may still follow them.
    /// Dropping this future after it starts retires the owned writer, even if a
    /// prefix was already accepted. Completion means writer acceptance, not
    /// encoder completion or presentation.
    pub async fn write_frame(&mut self, dimensions: (u32, u32), frame: &[u8]) -> io::Result<()> {
        if dimensions != self.dimensions || frame.len() != self.bytes {
            return Err(invalid("raw-frame geometry or byte count changed"));
        }
        let mut writer = self.writer.take().ok_or_else(retired)?;
        tokio::time::timeout(self.timeout, writer.write_all(frame))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "raw-frame writer stalled"))??;
        self.writer = Some(writer);
        Ok(())
    }

    /// End input within the same bounded budget. The writer stays retired even
    /// when shutdown fails or this future is cancelled.
    pub async fn shutdown(&mut self) -> io::Result<()> {
        let Some(mut writer) = self.writer.take() else {
            return Ok(());
        };
        tokio::time::timeout(self.timeout, writer.shutdown())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "raw-frame shutdown stalled"))?
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn retired() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "raw-frame writer retired; start a new encoder session",
    )
}

#[cfg(test)]
mod tests;
