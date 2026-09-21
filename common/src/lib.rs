//! Shared policy with independently selectable storage and platform adapters.
pub mod adb;
pub mod adb_reverse;
pub mod android;
pub mod camera;
pub mod display;
pub mod encoding;
pub mod idle;
pub mod model;
pub mod negotiation;
pub mod raw_frame;
pub mod release;
pub mod version;
pub mod video;
pub use model::*;

#[cfg(all(windows, feature = "storage"))]
pub mod windows;

#[cfg(feature = "storage")]
pub mod storage;
#[cfg(feature = "storage")]
pub use storage::config_path;

#[cfg(feature = "commands")]
pub mod commands;
#[cfg(feature = "commands")]
pub use commands::spawn_reaped;

#[cfg(feature = "platform")]
pub mod cli;
#[cfg(feature = "platform")]
pub mod platform;

#[cfg(all(target_os = "linux", feature = "platform"))]
pub mod linux;
#[cfg(all(target_os = "linux", feature = "platform"))]
pub use linux::{daemon_is_running, runtime};
