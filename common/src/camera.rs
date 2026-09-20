//! T539 independent camera profile; never changes display-stream settings.
use anyhow::{ensure, Result};
#[cfg(feature = "platform")]
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
mod preview;
pub use preview::CameraPreview;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "platform", derive(ValueEnum))]
#[serde(rename_all = "lowercase")]
pub enum Lens {
    #[default]
    Front,
    Rear,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraSettings {
    #[serde(flatten)]
    pub options: CameraProfile,
}

#[cfg_attr(feature = "platform", derive(Args))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraProfile {
    /// Lens selected by the desktop; Android still requires camera permission.
    #[cfg_attr(feature = "platform", arg(long, value_enum, default_value = "front"))]
    pub lens: Lens,
    /// Continue capture when the tablet app is in the background or locked.
    #[cfg_attr(feature = "platform", arg(long))]
    pub background: bool,
    /// ADB serial; required when more than one device is connected.
    #[cfg_attr(feature = "platform", arg(long))]
    pub serial: Option<String>,
    #[cfg_attr(feature = "platform", arg(long, default_value_t = 1280))]
    pub width: u32,
    #[cfg_attr(feature = "platform", arg(long, default_value_t = 720))]
    pub height: u32,
    #[cfg_attr(feature = "platform", arg(long, default_value_t = 30))]
    pub fps: u32,
    /// Camera H.264 target bitrate in kbit/s; independent of display bitrate.
    #[cfg_attr(feature = "platform", arg(long, default_value_t = 3000))]
    pub bitrate: u32,
    /// Additional clockwise rotation after Android sensor/display correction.
    #[cfg_attr(feature = "platform", arg(long, default_value_t = 0))]
    pub rotation: u16,
    /// Mirror exported camera video horizontally.
    #[cfg_attr(feature = "platform", arg(long))]
    pub mirror: bool,
}

/// Portable UI/backend contract. Only the native adapter owns OS devices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraState {
    Stopped,
    Starting,
    Waiting,
    Streaming,
    Stopping,
    Failed(String),
}

pub type BackendResult = Result<()>;

pub trait CameraBackend {
    fn start(&self, options: CameraProfile) -> Result<()>;
    fn stop(&self);
    fn state(&self) -> CameraState;
    fn preview(&self) -> Option<Arc<CameraPreview>>;
}

impl Default for CameraProfile {
    fn default() -> Self {
        Self {
            lens: Lens::Front,
            background: false,
            serial: None,
            width: 1280,
            height: 720,
            fps: 30,
            bitrate: 3000,
            mirror: false,
            rotation: 0,
        }
    }
}

impl CameraProfile {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.rotation, 0 | 90 | 180 | 270),
            "camera rotation must be 0, 90, 180 or 270 degrees"
        );
        ensure!(
            (160..=1920).contains(&self.width),
            "camera width must be 160–1920"
        );
        ensure!(
            (120..=1080).contains(&self.height),
            "camera height must be 120–1080"
        );
        ensure!(
            self.width.is_multiple_of(2) && self.height.is_multiple_of(2),
            "camera dimensions must be even"
        );
        ensure!((5..=30).contains(&self.fps), "camera FPS must be 5–30");
        ensure!(
            (256..=20000).contains(&self.bitrate),
            "camera bitrate must be 256–20000 kbit/s"
        );
        Ok(())
    }

    pub fn frame_bytes(&self) -> usize {
        (self.width * self.height * 3 / 2) as usize
    }
}

// Diagnostic CLI options belong to the native adapter, not the saved profile.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::CameraOptions;
#[cfg(not(target_os = "linux"))]
pub type CameraOptions = CameraProfile;

#[cfg(all(test, feature = "platform"))]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands};
    use clap::Parser;

    fn options() -> CameraOptions {
        let Some(Commands::Cameras(options)) = Cli::parse_from(["uscreen", "cameras"]).command
        else {
            panic!()
        };
        options
    }

    #[test]
    fn t539_profile_limits_and_bounded_invalid_inputs() {
        let defaults = options();
        for rotation in [0, 1, 89, 90, 180, 270, 360, u16::MAX] {
            let mut candidate = defaults.clone();
            candidate.rotation = rotation;
            assert_eq!(
                candidate.validate().is_ok(),
                [0, 90, 180, 270].contains(&rotation)
            );
        }
        defaults.validate().unwrap();
        assert_eq!(defaults.frame_bytes(), 1280 * 720 * 3 / 2);
        for value in [0, 1, 159, 160, 161, 720, 1080, 1280, 1920, 1921, u32::MAX] {
            let mut candidate = defaults.clone();
            candidate.width = value;
            assert_eq!(
                candidate.validate().is_ok(),
                (160..=1920).contains(&value) && value.is_multiple_of(2)
            );
            candidate = defaults.clone();
            candidate.height = value;
            assert_eq!(
                candidate.validate().is_ok(),
                (120..=1080).contains(&value) && value.is_multiple_of(2)
            );
        }
        for value in 0..64 {
            let mut candidate = defaults.clone();
            candidate.fps = value;
            assert_eq!(candidate.validate().is_ok(), (5..=30).contains(&value));
        }
        for value in [0, 255, 256, 3000, 20000, 20001, u32::MAX] {
            let mut candidate = defaults.clone();
            candidate.bitrate = value;
            assert_eq!(candidate.validate().is_ok(), (256..=20000).contains(&value));
        }
        #[cfg(target_os = "linux")]
        {
            let mut same = defaults;
            same.rear_device = same.front_device.clone();
            assert!(same.validate().is_err());
        }
    }
}
