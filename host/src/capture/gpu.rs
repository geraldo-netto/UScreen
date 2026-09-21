//! Opt-in Linux X11/VAAPI process adapter; portable sessions only see packets.
use super::CaptureConfig;
use std::{ffi::OsStr, path::PathBuf, process::Stdio};
use tokio::process::{Child, Command};

pub(super) struct Adapter {
    helper: Option<PathBuf>,
    active: bool,
    failed: bool,
    inspect: fn() -> (Vec<crate::vdisplay::EvdiConnector>, crate::desktop::Desktop),
}

fn native_target() -> (Vec<crate::vdisplay::EvdiConnector>, crate::desktop::Desktop) {
    (
        crate::vdisplay::evdi_connectors(),
        crate::desktop::Desktop::current(),
    )
}

impl Default for Adapter {
    fn default() -> Self {
        Self {
            helper: None,
            active: false,
            failed: false,
            inspect: native_target,
        }
    }
}

impl Adapter {
    pub(super) fn from_environment() -> Self {
        Self {
            // An explicit executable path is the prototype opt-in. Absence is
            // the ordinary FIFO path; no process-wide environment mutation.
            helper: std::env::var_os("USCREEN_X11_GPU_HELPER").map(PathBuf::from),
            ..Default::default()
        }
    }

    pub(super) fn try_start(
        &mut self,
        config: &CaptureConfig,
        mode: (u32, u32),
        card: Option<u32>,
    ) -> Option<Child> {
        let (connectors, desktop) = (self.inspect)();
        self.try_start_using(config, mode, card, &connectors, desktop)
    }

    fn try_start_using(
        &mut self,
        config: &CaptureConfig,
        mode: (u32, u32),
        card: Option<u32>,
        connectors: &[crate::vdisplay::EvdiConnector],
        desktop: crate::desktop::Desktop,
    ) -> Option<Child> {
        if self.failed {
            return None;
        }
        let program = self.helper.as_ref()?;
        let result = self.start_with(program.as_os_str(), config, mode, card, connectors, desktop);
        match result {
            Ok(child) => {
                self.active = true;
                tracing::info!("Experimental X11 GPU capture started; EVDI monitor owner retained");
                Some(child)
            }
            Err(error) => {
                self.failed = true;
                tracing::warn!("GPU capture unavailable ({error}); retaining EVDI/FIFO");
                None
            }
        }
    }

    fn start_with(
        &self,
        program: &OsStr,
        config: &CaptureConfig,
        mode: (u32, u32),
        card: Option<u32>,
        connectors: &[crate::vdisplay::EvdiConnector],
        desktop: crate::desktop::Desktop,
    ) -> anyhow::Result<Child> {
        let connector = supported_connector(config, card, connectors, desktop)?;
        command(program, config, mode, &connector)
            .spawn()
            .map_err(Into::into)
    }

    /// A native EOF/error is a one-way fallback for this session owner. No
    /// repeated GPU restart loop, monitor detach, or desktop reconfiguration.
    pub(super) fn encoder_finished(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.active = false;
        self.failed = true;
        tracing::warn!("GPU capture ended; resuming EVDI/FIFO without detaching the display");
        true
    }

    pub(super) fn stopped(&mut self) {
        self.active = false;
    }
}

fn supported_connector(
    config: &CaptureConfig,
    card: Option<u32>,
    connectors: &[crate::vdisplay::EvdiConnector],
    desktop: crate::desktop::Desktop,
) -> anyhow::Result<String> {
    anyhow::ensure!(desktop == crate::desktop::Desktop::X11, "requires X11");
    anyhow::ensure!(
        config.encoder == "h264_vaapi_baseline",
        "requires H.264 VAAPI Baseline"
    );
    anyhow::ensure!(
        !config.adaptive_idle && !config.ten_bit,
        "requires ordinary cadence and 8-bit output"
    );
    let card = card.ok_or_else(|| anyhow::anyhow!("no owned EVDI card"))?;
    let mut matches = connectors.iter().filter(|c| c.card == card && c.connected);
    let connector = matches
        .next()
        .ok_or_else(|| anyhow::anyhow!("owned connector absent"))?;
    anyhow::ensure!(matches.next().is_none(), "ambiguous owned connector");
    Ok(format!(
        "/sys/class/drm/card{}-{}/edid",
        card, connector.name
    ))
}

fn command(program: &OsStr, config: &CaptureConfig, mode: (u32, u32), connector: &str) -> Command {
    let mut command = Command::new(program);
    command
        .arg(connector)
        .arg(&config.vaapi_device)
        .args([
            mode.0.to_string(),
            mode.1.to_string(),
            config.stream_scale.to_string(),
            config.fps.to_string(),
            config.quality.to_string(),
            config.bitrate.to_string(),
            "0".into(),
            "desktop".into(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    command
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;
