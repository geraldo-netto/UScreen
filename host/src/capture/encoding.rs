//! Encoder process/task ownership. The supervisor decides when to start and stop.
use super::{process, CaptureConfig};
use crate::media::CodecConfig;
use anyhow::Result;
use std::sync::Arc;
use tokio::process::Child;

#[derive(Default)]
pub(super) struct EncoderProcess {
    pub(super) child: Option<Child>,
}

pub(super) struct EncoderOutput {
    pub(super) tx: crate::video_queue::VideoSender,
    pub(super) codec_config: CodecConfig,
    pub(super) latency: crate::latency::LatencyTracker,
    #[cfg_attr(not(feature = "inproc-encoder"), allow(dead_code))]
    pub(super) idr_wanted: Arc<std::sync::atomic::AtomicBool>,
}

impl EncoderProcess {
    pub(super) fn is_running(&self) -> bool {
        self.child.is_some()
    }

    #[cfg(not(feature = "inproc-encoder"))]
    pub(super) async fn start(
        &mut self,
        config: &CaptureConfig,
        mode: (u32, u32),
    ) -> Result<(u32, u32)> {
        let encoder = crate::config::ffmpeg_encoder_name(&config.encoder);
        let async_depth =
            super::cli_encoder::supports_async_depth(std::ffi::OsStr::new("ffmpeg"), encoder).await;
        self.start_with(config, mode, async_depth, |mut command| command.spawn())
    }

    #[cfg(feature = "inproc-encoder")]
    pub(super) async fn start(
        &mut self,
        config: &CaptureConfig,
        mode: (u32, u32),
    ) -> Result<(u32, u32)> {
        crate::config::validate_encoder_for_build(&config.encoder)?;
        Ok(mode)
    }

    #[cfg(not(feature = "inproc-encoder"))]
    pub(super) fn start_with(
        &mut self,
        config: &CaptureConfig,
        mode: (u32, u32),
        async_depth_supported: bool,
        spawn: impl FnOnce(tokio::process::Command) -> std::io::Result<Child>,
    ) -> Result<(u32, u32)> {
        use anyhow::Context;
        let adapter = super::cli_encoder::CliEncoder { config };
        let (w, h) = mode;
        adapter.log_encoder_dimensions(w, h);
        let child = spawn(adapter.encoder_command(w, h, async_depth_supported)?)
            .context("Failed to spawn ffmpeg encoder")?;
        tracing::info!("Encoder started (PID: {})", child.id().unwrap_or(0));
        self.child = Some(child);
        Ok(mode)
    }

    #[cfg(not(feature = "inproc-encoder"))]
    pub(super) fn spawn_session(
        &mut self,
        config: &CaptureConfig,
        mode: Option<(u32, u32)>,
        output: EncoderOutput,
    ) -> Result<EncoderTask> {
        let stdout = self
            .child
            .as_mut()
            .unwrap()
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Encoder has no stdout"))?;
        let codec = crate::media::Codec::from_encoder(&config.encoder);
        // Unknown dimensions cannot certify a matching automatic trial.
        let (w, h) = mode.unwrap_or((0, 0));
        let evidence = output.latency.encoder_started_with_decoder(
            &config.encoder,
            (w, h, config.fps, config.bitrate, config.quality),
            config.decoder.clone(),
        );
        let idle = super::idle::start(
            config,
            evidence.clone(),
            output.latency.clone(),
            output.tx.viewer_epoch(),
        );
        let handle = tokio::spawn(super::cli_encoder::read_loop_with_idle(
            stdout,
            output.tx,
            output.codec_config,
            output.latency,
            codec,
            evidence,
            idle,
        ));
        Ok(EncoderTask { handle })
    }

    #[cfg(feature = "inproc-encoder")]
    pub(super) fn spawn_session(
        &mut self,
        config: &CaptureConfig,
        mode: Option<(u32, u32)>,
        output: EncoderOutput,
    ) -> Result<EncoderTask> {
        let (w, h) = mode.expect("encoder size was selected before starting");
        let (name, fps, bitrate, quality) = (
            config.encoder.clone(),
            config.fps,
            config.bitrate,
            config.quality,
        );
        if config.ten_bit {
            tracing::warn!(
                "10-bit is not supported by the in-process encoder — \
                encoding 8-bit. Build without --features inproc-encoder for 10-bit."
            );
        }
        let fifo = super::fifo_path_for(config.instance)?;
        let stop = crate::encoder_io::StopSignal::new()?;
        let stopc = stop.clone();
        let handle = tokio::task::spawn_blocking(move || {
            crate::encoder::run(
                &fifo,
                &name,
                w,
                h,
                fps,
                bitrate,
                quality,
                output.tx,
                output.codec_config,
                output.idr_wanted,
                stopc,
                output.latency,
            )
        });
        Ok(EncoderTask { handle, stop })
    }

    pub(super) fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }

    pub(super) async fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            process::terminate(&mut child, "ffmpeg").await;
        }
    }
}

pub(super) struct EncoderTask {
    pub(super) handle: tokio::task::JoinHandle<Result<()>>,
    #[cfg(feature = "inproc-encoder")]
    stop: Arc<crate::encoder_io::StopSignal>,
}

impl EncoderTask {
    pub(super) fn abort(&mut self) {
        #[cfg(feature = "inproc-encoder")]
        self.stop.request();
        self.handle.abort();
    }

    pub(super) async fn finish(&mut self, already_finished: bool) {
        #[cfg(feature = "inproc-encoder")]
        self.stop.request();
        #[cfg(not(feature = "inproc-encoder"))]
        self.handle.abort();
        // Observe task destruction before rebuilding: the packetizer's
        // generation guard must retire every queued old access unit first.
        if !already_finished {
            let _ = (&mut self.handle).await;
        }
        self.handle.abort();
    }
}

// Blocking tasks cannot be aborted. Retire their reader on cancellation too.
#[cfg(feature = "inproc-encoder")]
impl Drop for EncoderTask {
    fn drop(&mut self) {
        self.stop.request();
    }
}
