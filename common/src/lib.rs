//! Shared policy with independently selectable storage and platform adapters.
pub mod adb;
pub mod display;
pub mod encoding;
pub mod model;
pub mod negotiation;
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
pub mod platform;

#[cfg(all(target_os = "linux", feature = "platform"))]
pub mod linux;
#[cfg(all(target_os = "linux", feature = "platform"))]
pub use linux::{daemon_is_running, runtime};
