//! Reusable host resource sharing, independent of display capture.
#[cfg(target_os = "linux")]
pub mod camera;
pub mod camera_control;
