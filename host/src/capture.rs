//! Capture supervision: settings, startup cancellation, retry and teardown ordering.
#[cfg(not(feature = "inproc-encoder"))]
mod cli_encoder;
mod config;
mod encoding;
mod fifo;
mod helper;
mod placement;
#[cfg(not(feature = "inproc-encoder"))]
pub(crate) mod probe;
#[cfg(not(feature = "inproc-encoder"))]
mod probe_format;
#[cfg(not(feature = "inproc-encoder"))]
mod probe_quality;
mod process;

use crate::media::CodecConfig;
use crate::media::EncoderSettings;
#[cfg(test)]
use crate::media::VideoPacket;
#[cfg(all(test, not(feature = "inproc-encoder")))]
use crate::media_storage::MediaBytes as Bytes;
use crate::runtime::fifo_path_for;
use anyhow::Result;
pub use config::CaptureConfig;
use encoding::{EncoderOutput, EncoderProcess};
use helper::{DetectedMode, HelperProcess};
use std::sync::Arc;
use std::time::Instant;
#[cfg(test)]
use tokio::sync::broadcast;
use tokio::sync::watch;
use tracing::{error, info, warn};
const RECONNECT_DELAY_MS: u64 = 2000;

pub struct CaptureManager {
    config: CaptureConfig,
    helper: HelperProcess,
    encoder: EncoderProcess,
    codec_config: CodecConfig,
    latency: crate::latency::LatencyTracker,
    idr_wanted: Arc<std::sync::atomic::AtomicBool>,
}
impl CaptureManager {
    pub fn new(config: CaptureConfig) -> Self {
        Self {
            config,
            helper: HelperProcess::new(),
            encoder: EncoderProcess::default(),
            codec_config: CodecConfig::default(),
            latency: crate::latency::LatencyTracker::new(),
            idr_wanted: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    async fn start_helper(&mut self) -> Result<()> {
        self.helper.start(&self.config).await
    }
    fn active_mode(&self) -> (u32, u32) {
        self.helper.active_mode(&self.config)
    }
    async fn start_session_encoder(&mut self) -> Result<(u32, u32)> {
        self.helper.recover_fifo()?;
        self.encoder.start(&self.config, self.active_mode()).await
    }
    /// Which EVDI card the helper opened; None until it has.
    pub fn card_rx(&self) -> watch::Receiver<Option<u32>> {
        self.helper.card_tx.subscribe()
    }

    /// Shared with the input server, which receives the tablet's render
    /// acknowledgements and closes the measurement loop.
    pub fn latency_tracker(&self) -> crate::latency::LatencyTracker {
        self.latency.clone()
    }

    /// Shared with the stream server so a connecting client can ask for a
    /// keyframe rather than waiting for one.
    #[cfg_attr(not(feature = "inproc-encoder"), allow(dead_code))]
    pub fn idr_request_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.idr_wanted.clone()
    }

    pub fn codec_config(&self) -> CodecConfig {
        self.codec_config.clone()
    }

    async fn while_active<T>(
        display: &mut watch::Receiver<bool>,
        shutdown: &mut watch::Receiver<bool>,
        operation: impl std::future::Future<Output = T>,
    ) -> Option<T> {
        tokio::pin!(operation);
        loop {
            if !*display.borrow()
                || *shutdown.borrow()
                || display.has_changed().is_err()
                || shutdown.has_changed().is_err()
            {
                return None;
            }
            tokio::select! {
                biased;
                _ = shutdown.changed() => {},
                _ = display.changed() => {},
                result = &mut operation => return Some(result),
            }
        }
    }

    async fn while_settings_current<T>(
        settings: &mut watch::Receiver<EncoderSettings>,
        display: &mut watch::Receiver<bool>,
        shutdown: &mut watch::Receiver<bool>,
        operation: impl std::future::Future<Output = T>,
        helper_only: bool,
    ) -> Option<T> {
        let initial = settings.borrow().clone();
        let mut updated = false;
        tokio::pin!(operation);
        loop {
            tokio::select! {
                biased;
                changed = settings.changed() => {
                    if changed.is_err() { return None; }
                    updated = true;
                    let current = settings.borrow();
                    let different = if helper_only {
                        current.helper_geometry() != initial.helper_geometry()
                            || !current.geometry_ready
                    } else { !current.same_stream(&initial) };
                    if different { return None; }
                }
                result = Self::while_active(display, shutdown, &mut operation) => {
                    if updated { settings.mark_changed(); }
                    return result;
                },
            }
        }
    }

    pub async fn stream_frames(
        &mut self,
        tx: crate::video_queue::VideoSender,
        settings_rx: watch::Receiver<EncoderSettings>,
        display_rx: watch::Receiver<bool>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Result<()> {
        let mut run = CaptureRun {
            settings_rx,
            display_rx,
            shutdown_rx,
            mode_rx: self.helper.mode_rx.clone(),
            stream_rx: self.helper.stream_rx.clone(),
            fifo_reset_rx: self.helper.fifo_reset_rx.clone(),
            backoff_ms: RECONNECT_DELAY_MS,
            explained_evdi: false,
            pipeline_started_at: Instant::now(),
            encoder_mode: None,
        };
        loop {
            if run.stopped() {
                self.shutdown().await;
                return Ok(());
            }
            self.apply_stream_settings(&mut run).await;
            if !*run.display_rx.borrow() || !run.settings_rx.borrow().geometry_ready {
                if self.idle_until_screen_change(&mut run).await {
                    return Ok(());
                }
                continue;
            }
            if !self.prepare_capture_pipeline(&mut run).await {
                continue;
            }
            let Some(changes) = self.run_encoder_session(&tx, &mut run).await? else {
                return Ok(());
            };
            run.adjust_backoff(changes);
            self.finish_encoder_session(changes, &mut run).await?;
        }
    }

    fn helper_settings_changed(&self, settings: &EncoderSettings) -> bool {
        settings.fps != self.config.fps
            || settings.width != self.config.width
            || settings.height != self.config.height
            || settings.width_mm != self.config.width_mm
            || settings.height_mm != self.config.height_mm
            || settings.stream_scale != self.config.stream_scale
    }

    fn stream_settings_changed(&self, settings: &EncoderSettings) -> bool {
        !settings.geometry_ready
            || self.helper_settings_changed(settings)
            || settings.effective_encoder() != self.config.encoder
            || settings.decoder_choice() != self.config.decoder.as_ref()
            || settings.bitrate != self.config.bitrate
            || settings.quality != self.config.quality
    }

    async fn apply_stream_settings(&mut self, run: &mut CaptureRun) {
        let s = run.settings_rx.borrow_and_update().clone();
        // The physical size is baked into the EDID alongside the mode,
        // so a change there needs a fresh helper too.
        let needs_helper_restart = self.helper_settings_changed(&s) && self.helper.is_running();
        if needs_helper_restart {
            // fps is baked into the helper's pacing, and the
            // resolution into the EDID — restart with a fresh EDID
            info!(
                "Display mode change: {}x{}@{} → {}x{}@{}",
                self.config.width, self.config.height, self.config.fps, s.width, s.height, s.fps
            );
            self.helper.terminate().await;
            // Give the compositor a moment to process the unplug
            Self::while_active(
                &mut run.display_rx,
                &mut run.shutdown_rx,
                tokio::time::sleep(std::time::Duration::from_millis(500)),
            )
            .await;
        }
        self.config.encoder = s.effective_encoder().to_string();
        self.config.decoder = s.decoder_choice().cloned();
        self.config.fps = s.fps;
        self.config.bitrate = s.bitrate;
        self.config.width = s.width;
        self.config.height = s.height;
        self.config.quality = s.quality;
        self.config.width_mm = s.width_mm;
        self.config.height_mm = s.height_mm;
        self.config.stream_scale = s.stream_scale;
    }

    /// Keep the virtual monitor disconnected until a tablet uses it as a screen.
    async fn idle_until_screen_change(&mut self, run: &mut CaptureRun) -> bool {
        if self.helper.is_running() {
            info!("No tablet is a screen — disconnecting the virtual display");
            self.helper.terminate().await;
        }
        self.encoder.stop();
        run.encoder_mode = None;
        tokio::select! {
            _ = run.display_rx.changed() => {}
            _ = run.settings_rx.changed() => {}
            _ = run.shutdown_rx.changed() => {
                self.shutdown().await;
                return true;
            }
        }
        false
    }

    async fn ensure_capture_helper(&mut self, run: &mut CaptureRun) -> bool {
        if self.helper.is_running() {
            return true;
        }
        let Some(result) = Self::while_settings_current(
            &mut run.settings_rx,
            &mut run.display_rx,
            &mut run.shutdown_rx,
            self.start_helper(),
            true,
        )
        .await
        else {
            return false;
        };
        if let Err(e) = result {
            error!(
                "Failed to start helper: {}. Retrying in {}ms...",
                e, run.backoff_ms
            );
            // Say why, once, instead of repeating an opaque line
            // forever. Retrying is right for a transient failure and
            // useless for a permissions problem, and the two look
            // identical from here without asking.
            run.explain_evdi_failure();
            run.back_off().await;
            return false;
        }
        run.explained_evdi = false;
        run.pipeline_started_at = Instant::now();
        true
    }

    async fn prepare_capture_pipeline(&mut self, run: &mut CaptureRun) -> bool {
        if !self.ensure_capture_helper(run).await || run.settings_rx.has_changed().unwrap_or(true) {
            return false;
        }
        // Enable the display via kscreen-doctor so KWin actively renders
        // to it (which is what makes evdi_grab_pixels produce anything).
        //
        // Only while a tablet is attached and being used as a screen:
        // enabling it unconditionally puts a monitor on the desktop that
        // nobody can see, and KDE happily moves windows onto it. The
        // run.display_rx branch below enables it the moment that changes.
        if Self::while_settings_current(
            &mut run.settings_rx,
            &mut run.display_rx,
            &mut run.shutdown_rx,
            placement::enable_evdi_display(self.helper.card, self.config.position),
            false,
        )
        .await
        .is_none()
        {
            return false;
        }

        // Wait (briefly) for the helper to report the mode the compositor
        // settled on before configuring ffmpeg's frame size. Guessing here
        // and getting it wrong yields a skewed picture for the whole
        // session, so a short wait is cheap insurance.
        //
        // Skipped when nothing is using the virtual output: it is
        // disabled then, so no mode is ever reported and the wait would
        // just add three seconds and a warning to every daemon start.
        if Self::while_settings_current(
            &mut run.settings_rx,
            &mut run.display_rx,
            &mut run.shutdown_rx,
            self.helper.wait_stream_size(&self.config),
            false,
        )
        .await
        .is_none()
        {
            return false;
        }
        run.mode_rx.borrow_and_update();
        run.stream_rx.borrow_and_update();

        self.ensure_session_encoder(run).await
    }

    async fn ensure_session_encoder(&mut self, run: &mut CaptureRun) -> bool {
        if self.encoder.is_running() {
            return true;
        }
        let Some(result) = Self::while_settings_current(
            &mut run.settings_rx,
            &mut run.display_rx,
            &mut run.shutdown_rx,
            self.start_session_encoder(),
            false,
        )
        .await
        else {
            return false;
        };
        match result {
            Ok(mode) => run.encoder_mode = Some(mode),
            Err(e) => {
                error!(
                    "Failed to start encoder: {}. Retrying in {}ms...",
                    e, run.backoff_ms
                );
                run.back_off().await;
                return false;
            }
        }
        true
    }

    async fn run_encoder_session(
        &mut self,
        tx: &crate::video_queue::VideoSender,
        run: &mut CaptureRun,
    ) -> Result<Option<SessionChanges>> {
        let output = EncoderOutput {
            tx: tx.clone(),
            codec_config: self.codec_config.clone(),
            latency: self.latency.clone(),
            idr_wanted: self.idr_wanted.clone(),
        };
        let mut encode_task = self
            .encoder
            .spawn_session(&self.config, run.encoder_mode, output)?;

        let mut settings_changed = false;
        let mut fifo_reset = false;
        // Distinct from `settings_changed`: the mode moved under us, so the
        // encoder must be rebuilt but the helper and the virtual display
        // are fine and must not be torn down.
        let mut mode_changed = false;
        // The tablet stopped being a screen: a clean stop, not a crash,
        // so no backoff and no wait before the outer loop parks itself.
        let mut display_dropped = false;
        let card = self.helper.card;

        // Events that need no restart at all send us back here without
        // rebuilding the encoder, which now owns the stream and must not be
        // torn down for something spurious.
        #[allow(unused_labels)]
        'session: loop {
            let mut resume_same_encoder = false;
            let mut encode_finished = false;
            tokio::select! {
                status = self.helper.wait() => {
                    warn!("Capture helper exited: {:?}. Restarting...", status);
                }
                joined = &mut encode_task.handle => {
                    encode_finished = true;
                    match joined {
                        Ok(Ok(_)) => info!("Encoder finished"),
                        Ok(Err(e)) => warn!("Encoder error: {}. Restarting...", e),
                        Err(e) => warn!("Encoder task failed: {}. Restarting...", e),
                    }
                }
                _ = run.fifo_reset_rx.changed() => {
                    if self.helper.fifo_reset_pending(self.config.instance)? {
                        warn!("Partial capture frame — restarting reader on a fresh FIFO");
                        fifo_reset = true;
                    } else {
                        resume_same_encoder = true;
                    }
                }
                changed = run.settings_rx.changed() => {
                    settings_changed = changed.is_err() || self.stream_settings_changed(&run.settings_rx.borrow_and_update());
                    resume_same_encoder = !settings_changed;
                }
                _ = run.stream_rx.changed() => {
                    let now = self.active_mode();
                    if Some(now) == run.encoder_mode {
                        resume_same_encoder = true;
                    } else {
                        info!("Stream size is now {}x{} — restarting encoder", now.0, now.1);
                        mode_changed = true;
                    }
                }
                _ = run.mode_rx.changed() => {
                    let now = self.active_mode();
                    if Some(now) == run.encoder_mode {
                        // KWin re-applying the same mode. Nothing to do.
                        resume_same_encoder = true;
                    } else {
                        info!(
                            "Virtual output changed to {}x{} — restarting encoder to match",
                            now.0, now.1
                        );
                        mode_changed = true;
                    }
                }
                _ = run.display_rx.changed() => {
                    // The tablet stopped being a screen — either unplugged, or
                    // switched to pen-only. Neither must leave a monitor behind
                    // that nobody can see, with windows stranded on it. The
                    // encoder itself is unaffected either way.
                    let wanted = *run.display_rx.borrow();
                    if wanted {
                        info!("Tablet is a screen — enabling the virtual display");
                        Self::while_active(&mut run.display_rx, &mut run.shutdown_rx,
                            placement::enable_evdi_display(card, self.config.position)).await;
                        // Leave cancellation pending for the session select.
                        if *run.shutdown_rx.borrow() || run.shutdown_rx.has_changed().is_err() {
                            encode_task.abort();
                            self.shutdown().await;
                            return Ok(None);
                        }
                        if !*run.display_rx.borrow() || run.display_rx.has_changed().is_err() {
                            display_dropped = true;
                        } else {
                            resume_same_encoder = true;
                        }
                    } else {
                        // Disable first so KWin moves the windows off it, then
                        // fall through to the teardown: the helper goes away
                        // with the session, and the top of the outer loop
                        // waits for a tablet before bringing anything back.
                        info!("Tablet is not a screen — disabling the virtual display");
                        let _ = tokio::time::timeout(std::time::Duration::from_millis(500),
                            placement::disable_evdi_display(card)).await;
                        display_dropped = true;
                    }
                }
                _ = run.shutdown_rx.changed() => {
                    info!("Shutdown requested — tearing down the capture pipeline");
                    // Drop the encode task, which closes our read end of the
                    // pipe. Nothing drains it during shutdown, so ffmpeg would
                    // otherwise block writing into a full pipe and never reach
                    // its signal handling — a wasted 1.5s SIGTERM timeout on
                    // every stop. Closed, it gets EPIPE and exits at once.
                    encode_task.abort();
                    let _ = tokio::time::timeout(std::time::Duration::from_millis(500),
                            placement::disable_evdi_display(card)).await;
                    self.shutdown().await;
                    return Ok(None);
                }
            }

            if resume_same_encoder {
                // Nothing about the encoder changed, so it keeps running and we
                // simply go back to waiting on it. The task owns the stream, so
                // it must not be torn down and rebuilt for a spurious event.
                continue 'session;
            }

            // Retire the reader before the supervisor rebuilds the pipeline.
            encode_task.finish(encode_finished).await;

            break;
        }

        Ok(Some(SessionChanges {
            settings_changed,
            mode_changed,
            display_dropped,
            fifo_reset,
        }))
    }

    async fn finish_encoder_session(
        &mut self,
        changes: SessionChanges,
        run: &mut CaptureRun,
    ) -> Result<()> {
        // Clean up and retry. On a settings or mode change, keep the helper
        // alive (an fps/resolution change is handled at the top of the loop)
        // so the virtual display doesn't flicker off.
        if !changes.keep_helper() {
            self.helper.terminate().await;
        }
        // Every replacement reader needs a whole-frame boundary, even when
        // the writer has not yet reported a partial write. Reap before rotation.
        self.encoder.shutdown().await;
        if changes.keep_helper() {
            self.helper.rotate_fifo()?;
        }
        run.encoder_mode = None;
        // Reset codec config so it gets re-extracted on restart
        self.codec_config.publish(None);
        if changes.crashed() {
            run.pause(run.backoff_ms).await;
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        self.helper.stop();
        self.encoder.stop();
    }

    /// Stop the pipeline and wait for the children to actually be gone.
    ///
    /// The synchronous [`stop`] only *sends* signals, so the daemon could exit
    /// while ffmpeg was still holding the capture FIFO. That window collides
    /// with an immediate restart — which is exactly what "Apply & restart" in
    /// the GUI does — and two processes on one FIFO produce torn frames.
    pub async fn shutdown(&mut self) {
        self.helper.abort_stdout();
        self.encoder.shutdown().await;
        self.helper.terminate().await;
        self.helper.fifo.take();
    }
}

/// Watches and retry state for one capture pipeline.
struct CaptureRun {
    settings_rx: watch::Receiver<EncoderSettings>,
    display_rx: watch::Receiver<bool>,
    shutdown_rx: watch::Receiver<bool>,
    mode_rx: watch::Receiver<Option<DetectedMode>>,
    stream_rx: watch::Receiver<Option<(u32, u32)>>,
    fifo_reset_rx: watch::Receiver<Option<fifo::Identity>>,
    backoff_ms: u64,
    explained_evdi: bool,
    pipeline_started_at: Instant,
    encoder_mode: Option<(u32, u32)>,
}

impl CaptureRun {
    fn stopped(&self) -> bool {
        *self.shutdown_rx.borrow()
            || self.shutdown_rx.has_changed().is_err()
            || self.display_rx.has_changed().is_err()
            || self.settings_rx.has_changed().is_err()
    }

    async fn pause(&mut self, milliseconds: u64) {
        CaptureManager::while_active(
            &mut self.display_rx,
            &mut self.shutdown_rx,
            tokio::time::sleep(std::time::Duration::from_millis(milliseconds)),
        )
        .await;
    }

    async fn back_off(&mut self) {
        self.pause(self.backoff_ms).await;
        self.backoff_ms = (self.backoff_ms * 2).min(30_000);
    }

    fn explain_evdi_failure(&mut self) {
        self.explain_evdi_failure_with(helper::evdi_setup_problem);
    }

    fn explain_evdi_failure_with(&mut self, inspect: impl FnOnce() -> Option<String>) {
        if !self.explained_evdi {
            if let Some(reason) = inspect() {
                self.explained_evdi = true;
                error!("{}", reason);
            }
        }
    }

    fn adjust_backoff(&mut self, changes: SessionChanges) {
        if self.pipeline_started_at.elapsed().as_secs() >= 30 {
            self.backoff_ms = RECONNECT_DELAY_MS;
        } else if changes.crashed() {
            self.backoff_ms = (self.backoff_ms * 2).min(30_000);
        }
    }
}

#[derive(Clone, Copy)]
struct SessionChanges {
    settings_changed: bool,
    mode_changed: bool,
    display_dropped: bool,
    fifo_reset: bool,
}
impl SessionChanges {
    fn keep_helper(self) -> bool {
        self.settings_changed || self.mode_changed || self.fifo_reset
    }
    fn crashed(self) -> bool {
        !self.keep_helper() && !self.display_dropped
    }
}

impl Drop for CaptureManager {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
#[path = "capture_card_tests.rs"]
mod card_allocation_tests;
#[cfg(all(test, feature = "inproc-encoder"))]
#[path = "capture/inproc_tests.rs"]
mod inproc_tests;
#[cfg(test)]
#[path = "capture/native_path_tests.rs"]
mod native_path_tests;
#[cfg(test)]
#[path = "capture/orphan_tests.rs"]
mod orphan_tests;
#[cfg(test)]
#[path = "capture/ownership_tests.rs"]
mod ownership_tests;
#[cfg(all(test, not(feature = "inproc-encoder")))]
#[path = "capture/tests.rs"]
mod tests;

#[cfg(all(test, not(feature = "inproc-encoder")))]
#[path = "capture/coverage_tests.rs"]
mod coverage_tests;

#[cfg(test)]
#[path = "capture/fifo_tests.rs"]
mod fifo_tests;

#[cfg(all(test, not(feature = "inproc-encoder")))]
#[path = "capture/closed_settings_tests.rs"]
mod closed_settings_tests;
