//! Reusable host resource sharing, independent of display capture.
#[cfg(target_os = "linux")]
pub mod camera;
pub mod camera_control;

// Shared wire/session state is built for every host target.
pub mod attachment;
pub mod config;
pub mod latency;
pub mod media;
pub mod media_storage;
#[path = "selection_types.rs"]
pub mod selection;
pub mod video_queue;

#[cfg(test)]
mod test_logging;

pub mod input;
pub mod stream;

// Linux display/input adapters are never linked into other host targets.
#[cfg(test)]
#[allow(dead_code)] // Shared probe also supports the binary packetizer benchmarks.
mod allocation_probe;
#[cfg(target_os = "linux")]
pub mod kscreen;
#[cfg(target_os = "linux")]
pub mod kwin;
#[cfg(target_os = "linux")]
pub mod osk;
#[cfg(test)]
mod poll_probe;
#[cfg(target_os = "linux")]
pub mod vdisplay;

#[path = "session_core.rs"]
pub mod session;
