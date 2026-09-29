//! Reusable host resource sharing, independent of display capture.
#[cfg(target_os = "linux")]
pub mod audio;
pub mod audio_control;
#[cfg(target_os = "linux")]
pub mod camera;
pub mod camera_control;

// Shared wire/session state is built for every host target.
pub mod adb_inventory;
pub mod attachment;
pub mod config;
pub mod ffmpeg_args;
pub mod latency;
pub mod media;
pub mod media_storage;
pub mod pipe_encoder;
pub mod raw_transfer;
#[path = "selection_types.rs"]
pub mod selection;
pub mod tray_state;
pub mod usb;
pub mod video_queue;
#[cfg(windows)]
pub mod windows_tray;

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

mod annex_b;
pub mod annex_scan;
pub mod encoder_probe;
mod framed_annex_b;
#[path = "capture/probe_format.rs"]
pub mod probe_format;

#[path = "monitor/launch_policy.rs"]
mod launch_policy;
pub mod transport;
pub mod wifi;

mod command_output;
pub mod update;
