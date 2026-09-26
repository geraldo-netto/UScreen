//! EVDI helper process, FIFO creation, negotiated geometry and card announcements.
use super::{fifo, fifo_path_for, process, CaptureConfig};
use anyhow::{Context, Result};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;
use tracing::{info, warn};
type HelperLines = tokio::io::Lines<BufReader<tokio::process::ChildStdout>>;

/// Why the helper cannot get an EVDI device, in the words of someone who can
/// fix it. Returns None when the state looks fine and the failure was
/// something else.
pub(super) fn evdi_setup_problem() -> Option<String> {
    evdi_setup_problem_in(std::path::Path::new("/sys/devices/evdi"))
}

/// Split out so it can be tested against a temporary directory. Unloading the
/// real module needs root and takes Xwayland down with it, so the three states
/// this has to tell apart are otherwise unreachable from a test.
fn evdi_setup_problem_in(dir: &std::path::Path) -> Option<String> {
    let module_loaded = dir.exists();
    if !module_loaded {
        return Some(
            "The evdi kernel module is not loaded. Install it (evdi-dkms; on Arch it is in \
             the AUR) and run: sudo modprobe evdi"
                .to_string(),
        );
    }

    let count: u32 = std::fs::read_to_string(dir.join("count"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    if count > 0 {
        return None;
    }

    // The module is there but has no devices, and creating one needs a write
    // to a root-only file. `initial_device_count` fixes it for good, but only
    // takes effect when the module loads — which is why writing the
    // modprobe.d file is not enough on a machine where evdi is already
    // resident.
    Some(
        "No EVDI device exists and /sys/devices/evdi/add is root-only, so one cannot be \
         created. Fix it for this boot with:\n    echo 1 | sudo tee /sys/devices/evdi/add\n\
         and for every boot with:\n    echo 'options evdi initial_device_count=2' | sudo tee \
         /etc/modprobe.d/blent-evdi.conf\n\
         This boot setting applies after reboot; preserve the loaded module.\n\
         Then run: blent doctor"
            .to_string(),
    )
}
/// The display mode KWin actually negotiated on the virtual output, as
/// reported by the helper's `MODE_CHANGED` line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DetectedMode {
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
}

pub(super) struct HelperProcess {
    pub(super) child: Option<Child>,
    pub(super) fifo: Option<fifo::Owned>,
    #[cfg(feature = "inproc-encoder")]
    pub(super) raw_socket: Option<crate::raw_socket::Socket>,
    stdout_task: Option<tokio::task::JoinHandle<()>>,
    /// Retained across restarts as a preference, never a reservation.
    pub(super) card: Option<u32>,
    pub(super) mode_tx: watch::Sender<Option<DetectedMode>>,
    pub(super) mode_rx: watch::Receiver<Option<DetectedMode>>,
    /// Actual emitted size, which may differ from the display mode when scaled.
    pub(super) stream_tx: watch::Sender<Option<(u32, u32)>>,
    pub(super) stream_rx: watch::Receiver<Option<(u32, u32)>>,
    fifo_reset_tx: watch::Sender<Option<fifo::Identity>>,
    pub(super) fifo_reset_rx: watch::Receiver<Option<fifo::Identity>>,
    pub(super) card_tx: watch::Sender<Option<u32>>,
}
impl HelperProcess {
    pub(super) fn new() -> Self {
        let (mode_tx, mode_rx) = watch::channel(None);
        let (fifo_reset_tx, fifo_reset_rx) = watch::channel(None);
        let (stream_tx, stream_rx) = watch::channel(None);
        Self {
            child: None,
            fifo: None,
            #[cfg(feature = "inproc-encoder")]
            raw_socket: None,
            stdout_task: None,
            card: None,
            mode_tx,
            mode_rx,
            stream_tx,
            stream_rx,
            card_tx: watch::channel(None).0,
            fifo_reset_tx,
            fifo_reset_rx,
        }
    }
    pub(super) fn fifo_reset_pending(&self, instance: u32) -> Result<bool> {
        match *self.fifo_reset_rx.borrow() {
            Some(retired) => retired.matches(&fifo_path_for(instance)?),
            None => Ok(false),
        }
    }
    pub(super) fn recover_fifo(&mut self) -> Result<()> {
        if let Some(retired) = *self.fifo_reset_rx.borrow() {
            self.fifo
                .as_mut()
                .context("no owned capture FIFO")?
                .replace_retired(retired)?;
        }
        Ok(())
    }
    pub(super) fn rotate_fifo(&mut self) -> Result<()> {
        self.fifo
            .as_mut()
            .context("no owned capture FIFO")?
            .rotate()
    }
    pub(super) fn is_running(&self) -> bool {
        self.child.is_some()
    }
    pub(super) async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        match self.child.as_mut() {
            Some(child) => child.wait().await,
            None => std::future::pending().await,
        }
    }
    pub(super) fn abort_stdout(&mut self) {
        if let Some(task) = self.stdout_task.take() {
            task.abort();
        }
    }
    pub(super) fn stop(&mut self) {
        self.abort_stdout();
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
    pub(super) async fn terminate(&mut self) {
        if let Some(mut child) = self.child.take() {
            process::terminate(&mut child, "evdi_helper").await;
        }
    }
    /// Frame size the encoder must be configured for.
    ///
    /// This is what the helper emits, which is the negotiated display mode
    /// divided by the stream scale — not the mode itself. Getting it wrong
    /// skews every frame for the whole session.
    pub(super) fn active_mode(&self, config: &CaptureConfig) -> (u32, u32) {
        if let Some((w, h)) = *self.stream_rx.borrow() {
            if w > 0 && h > 0 {
                return (w, h);
            }
        }
        let (w, h) = match *self.mode_rx.borrow() {
            Some(m) if m.width > 0 && m.height > 0 => (m.width, m.height),
            _ => (config.width, config.height),
        };
        let n = config.stream_scale.max(1);
        (((w / n) & !1).max(2), ((h / n) & !1).max(2))
    }

    pub(super) async fn wait_stream_size(&self, config: &CaptureConfig) {
        if self.stream_rx.borrow().is_none() {
            let mut wait_rx = self.stream_rx.clone();
            if tokio::time::timeout(
                std::time::Duration::from_secs(3),
                wait_rx.wait_for(Option::is_some),
            )
            .await
            .is_err()
            {
                warn!(
                    "Compositor reported no mode within 3s — encoding at the requested {}x{}",
                    config.width, config.height
                );
            }
        }
    }

    pub(super) async fn start(&mut self, config: &CaptureConfig) -> Result<()> {
        crate::config::validate_encoder_for_build(&config.encoder)?;
        if config.shared_raw() {
            blent_config::raw_frame::Layout::new(2, 2, config.raw_slots)?;
        }
        anyhow::ensure!(!config.shared_raw() || cfg!(feature = "inproc-encoder"), "shared_memory raw transport requires --features inproc-encoder; use fifo with stock FFmpeg CLI/VAAPI");
        let fifo = fifo_path_for(config.instance)?;
        self.fifo = Some(fifo::Owned::create(&fifo)?);
        process::retire_orphan_capture(&fifo).await?;
        let mut command = self.command(config, &fifo)?;
        #[cfg(feature = "inproc-encoder")]
        self.attach_raw_socket(config, &mut command)?;
        let mut child = command.spawn().context("Failed to spawn evdi-helper")?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("No stdout from helper"))?;
        let mut lines = BufReader::new(stdout).lines();
        let card = Self::await_helper_card(&mut lines).await?;
        self.card = Some(card);
        let _ = self.card_tx.send(Some(card));
        // A fresh helper has not negotiated a mode yet.
        let _ = self.mode_tx.send(None);
        let _ = self.fifo_reset_tx.send(None);
        let _ = self.stream_tx.send(None);
        self.abort_stdout();
        self.stdout_task = Some(tokio::spawn(Self::drain_helper_stdout(
            lines,
            self.mode_tx.clone(),
            self.stream_tx.clone(),
            self.fifo_reset_tx.clone(),
            blent_config::linux::pipe::ReportFile::new(
                fifo,
                child.id().context("helper has no PID")?,
            )?,
        )));
        self.child = Some(child);
        Ok(())
    }

    #[cfg(feature = "inproc-encoder")]
    fn attach_raw_socket(&mut self, config: &CaptureConfig, command: &mut Command) -> Result<()> {
        self.raw_socket = None;
        if config.shared_raw() {
            let (parent, child) = crate::raw_socket::Socket::pair()?;
            child.attach(command)?;
            self.raw_socket = Some(parent);
        }
        Ok(())
    }

    pub(super) fn command(&self, config: &CaptureConfig, fifo: &Path) -> Result<Command> {
        // EDID 1.4 pixel-clock field is 16-bit (max 655 MHz).
        // 2960×1848 @120 Hz needs ~706 MHz which overflows.
        // Cap the EDID at 90 Hz so KDE can render at 90 fps; the helper
        // captures at the configured fps independently via clock_nanosleep.
        let edid_fps = config.fps.min(90);
        let edid_path = match &config.edid_path {
            Some(p) => p.clone(),
            None => crate::edid::ensure_edid_sized(
                config.width,
                config.height,
                edid_fps,
                config.width_mm,
                config.height_mm,
            )?,
        };

        let mut cmd = Command::new(&config.helper_path);
        cmd.arg("--edid").arg(edid_path);
        cmd.args(["--fps", &config.fps.to_string()]);
        cmd.args([
            "--conversion-threads",
            &config.conversion_threads.to_string(),
        ]);
        if config.stream_scale > 1 {
            cmd.args(["--scale", &config.stream_scale.to_string()]);
        }

        cmd.arg("--capture-fifo").arg(fifo);
        if config.adaptive_idle && !cfg!(feature = "inproc-encoder") {
            cmd.arg("--idle-control-file")
                .arg(fifo.with_extension("idle"));
        }
        cmd.arg("--pipe-size-file")
            .arg(blent_config::linux::pipe::request_path()?);
        if let Some(card) = config.card {
            cmd.args(["--card", &card.to_string()]);
        } else if let Some(previous) = self.card {
            cmd.args(["--preferred-card", &previous.to_string()]);
        }

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        Ok(cmd)
    }

    async fn await_helper_card(lines: &mut HelperLines) -> Result<u32> {
        loop {
            match lines.next_line().await {
                Ok(Some(l)) => {
                    if let Some(rest) = l.strip_prefix("EVDI_CONNECTED card") {
                        let card = rest.trim().parse()?;
                        info!("Helper connected on card{}", card);
                        return Ok(card);
                    }
                }
                Ok(None) => {
                    anyhow::bail!("evdi-helper exited prematurely");
                }
                Err(e) => {
                    anyhow::bail!("evdi-helper stdout error: {}", e);
                }
            }
        }
    }

    // Retain stdout for the entire helper lifetime so negotiated mode reports
    // do not hit EPIPE after the initial connection handshake.
    async fn drain_helper_stdout(
        mut lines: HelperLines,
        mode_tx: watch::Sender<Option<DetectedMode>>,
        stream_tx: watch::Sender<Option<(u32, u32)>>,
        fifo_reset_tx: watch::Sender<Option<fifo::Identity>>,
        pipe_report: blent_config::linux::pipe::ReportFile,
    ) {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Err(error) = pipe_report.observe(&line) {
                warn!(%error, "Cannot update capture pipe status");
            }
            if let Some(retired) = fifo::Identity::from_reset_line(&line) {
                let _ = fifo_reset_tx.send(Some(retired));
            } else {
                Self::publish_helper_line(&line, &mode_tx, &stream_tx);
            }
        }
    }

    fn publish_helper_line(
        line: &str,
        mode_tx: &watch::Sender<Option<DetectedMode>>,
        stream_tx: &watch::Sender<Option<(u32, u32)>>,
    ) {
        if let Some(rest) = line.strip_prefix("STREAM_SIZE ") {
            let mut p = rest.split_whitespace();
            if let (Some(Ok(w)), Some(Ok(h))) = (
                p.next().map(str::parse::<u32>),
                p.next().map(str::parse::<u32>),
            ) {
                if w > 0 && h > 0 {
                    info!("Helper emits {}x{} frames to the encoder", w, h);
                    let _ = stream_tx.send(Some((w, h)));
                }
            }
            return;
        }
        let Some(rest) = line.strip_prefix("MODE_CHANGED ") else {
            return;
        };
        let mut parts = rest.split_whitespace();
        let parsed = (|| {
            Some(DetectedMode {
                width: parts.next()?.parse().ok()?,
                height: parts.next()?.parse().ok()?,
                refresh: parts.next()?.parse().ok()?,
            })
        })();
        let Some(mode) = parsed else {
            warn!("Unparseable MODE_CHANGED from helper: {}", rest);
            return;
        };
        if mode.width == 0 || mode.height == 0 {
            return;
        }
        info!(
            "Compositor negotiated {}x{}@{}Hz on the virtual output",
            mode.width, mode.height, mode.refresh
        );
        let _ = mode_tx.send(Some(mode));
    }
}

pub(super) fn ensure_fifo(path: &Path) -> Result<()> {
    // Anything already there is replaced, whatever it is: a stale FIFO
    // may carry old permissions, and a regular file or symlink in this
    // spot is not ours. mkfifo itself never follows symlinks.
    let _ = std::fs::remove_file(path);
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).context("fifo path")?;
    // 0600: the helper writes it and the encoder reads it, both as this
    // user. Nobody else has any business with a live copy of the screen.
    let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("mkfifo");
    }
    Ok(())
}

#[cfg(test)]
mod coverage_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn t418_unavailable_or_invalid_transport_fails_before_resources() {
        let mut helper = HelperProcess::new();
        let mut config = CaptureConfig {
            encoder: "libx264".into(),
            helper_path: "/nonexistent-t418-helper".into(),
            raw_transport: blent_config::raw_frame::RawTransport::SharedMemory,
            ..Default::default()
        };
        config.raw_slots = 9;
        assert!(helper
            .start(&config)
            .await
            .unwrap_err()
            .to_string()
            .contains("slot"));
        assert!(helper.fifo.is_none() && helper.child.is_none());
        #[cfg(not(feature = "inproc-encoder"))]
        {
            config.raw_slots = 4;
            assert!(helper
                .start(&config)
                .await
                .unwrap_err()
                .to_string()
                .contains("requires --features inproc-encoder"));
            assert!(helper.fifo.is_none() && helper.child.is_none());
        }
    }

    #[cfg(feature = "inproc-encoder")]
    #[test]
    fn t418_helper_transport_handoff_retains_only_parent_endpoint() {
        use std::os::fd::AsRawFd;
        let mut helper = HelperProcess::new();
        let mut config = CaptureConfig {
            raw_transport: blent_config::raw_frame::RawTransport::SharedMemory,
            ..Default::default()
        };
        let mut command = Command::new("/unused-helper");
        helper.attach_raw_socket(&config, &mut command).unwrap();
        let parent = helper.raw_socket.clone().unwrap();
        assert_ne!(
            unsafe { libc::fcntl(parent.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        assert!(parent.receive().unwrap().is_none());
        drop(command); // Partial startup retires the never-executed child endpoint.
        assert!(parent.receive().is_err());
        config.raw_transport = blent_config::raw_frame::RawTransport::Fifo;
        helper
            .attach_raw_socket(&config, &mut Command::new("/unused-helper"))
            .unwrap();
        assert!(helper.raw_socket.is_none());
    }
    #[test]
    fn evdi_problem_reports_a_missing_module() {
        let dir = std::env::temp_dir().join("blent-test-evdi-absent");
        let _ = std::fs::remove_dir_all(&dir);
        let msg = evdi_setup_problem_in(&dir).expect("absent module is a problem");
        assert!(msg.contains("not loaded"), "got: {msg}");
    }
    #[test]
    fn evdi_problem_reports_a_module_with_no_devices() {
        // The state the Arch report landed in: module resident, count 0, and
        // /sys/devices/evdi/add writable only by root.
        let dir = std::env::temp_dir().join("blent-test-evdi-empty");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("count"), "0\n").unwrap();
        let msg = evdi_setup_problem_in(&dir).expect("no devices is a problem");
        assert!(msg.contains("initial_device_count"), "got: {msg}");
        assert!(msg.contains("/sys/devices/evdi/add"), "T269: {msg}");
        assert!(msg.contains("reboot"), "T269: {msg}");
        assert!(
            !msg.contains("modprobe -r"),
            "T269: preserve live devices: {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn evdi_problem_stays_quiet_when_a_device_exists() {
        let dir = std::env::temp_dir().join("blent-test-evdi-ok");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("count"), "1\n").unwrap();
        assert!(
            evdi_setup_problem_in(&dir).is_none(),
            "a working setup must not be blamed for an unrelated failure"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
