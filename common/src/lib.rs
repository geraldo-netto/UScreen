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

#[cfg(feature = "storage")]
pub mod storage;
#[cfg(feature = "storage")]
pub use storage::config_path;

#[cfg(feature = "commands")]
pub mod commands;
#[cfg(feature = "commands")]
pub use commands::spawn_reaped;

#[cfg(all(target_os = "linux", feature = "platform-linux"))]
pub mod linux;
#[cfg(all(target_os = "linux", feature = "platform-linux"))]
pub use linux::{daemon_is_running, runtime};
