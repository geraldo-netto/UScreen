//! Linux diagnostic command adapter; saved camera profiles remain portable.
use super::CameraProfile;
use anyhow::{ensure, Result};
#[cfg(feature = "platform")]
use clap::Args;
use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "platform", derive(Args))]
pub struct CameraOptions {
    #[cfg_attr(feature = "platform", command(flatten))]
    pub profile: CameraProfile,
    /// Existing v4l2loopback device labelled Blent Front.
    #[cfg_attr(feature = "platform", arg(long, default_value = "/dev/video20"))]
    pub front_device: PathBuf,
    /// Existing v4l2loopback device labelled Blent Rear.
    #[cfg_attr(feature = "platform", arg(long, default_value = "/dev/video21"))]
    pub rear_device: PathBuf,
}
impl Default for CameraOptions {
    fn default() -> Self {
        Self {
            profile: CameraProfile::default(),
            front_device: "/dev/video20".into(),
            rear_device: "/dev/video21".into(),
        }
    }
}
impl Deref for CameraOptions {
    type Target = CameraProfile;
    fn deref(&self) -> &Self::Target {
        &self.profile
    }
}
impl DerefMut for CameraOptions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.profile
    }
}
impl CameraOptions {
    pub fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        ensure!(
            self.front_device != self.rear_device,
            "camera outputs must be distinct"
        );
        Ok(())
    }
}
