//! T539 independent camera profile; never changes display-stream settings.
use anyhow::{ensure, Result};
use clap::Args;
use std::path::PathBuf;

#[derive(Args, Clone, Debug)]
pub struct CameraOptions {
    /// ADB serial; required when more than one device is connected.
    #[arg(long)]
    pub serial: Option<String>,
    /// Existing v4l2loopback device labelled UScreen Front.
    #[arg(long, default_value = "/dev/video20")]
    pub front_device: PathBuf,
    /// Existing v4l2loopback device labelled UScreen Rear.
    #[arg(long, default_value = "/dev/video21")]
    pub rear_device: PathBuf,
    #[arg(long, default_value_t = 1280)]
    pub width: u32,
    #[arg(long, default_value_t = 720)]
    pub height: u32,
    #[arg(long, default_value_t = 30)]
    pub fps: u32,
    /// Camera H.264 target bitrate in kbit/s; independent of display bitrate.
    #[arg(long, default_value_t = 3000)]
    pub bitrate: u32,
    /// Mirror exported camera video horizontally.
    #[arg(long)]
    pub mirror: bool,
}

impl CameraOptions {
    pub fn validate(&self) -> Result<()> {
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
        ensure!(
            self.front_device != self.rear_device,
            "camera outputs must be distinct"
        );
        Ok(())
    }

    pub fn frame_bytes(&self) -> usize {
        (self.width * self.height * 3 / 2) as usize
    }
}

#[cfg(test)]
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
        let mut same = defaults;
        same.rear_device = same.front_device.clone();
        assert!(same.validate().is_err());
    }
}
