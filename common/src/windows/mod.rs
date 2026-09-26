//! Native Windows adapters shared by the library, host and GUI.
mod native;
pub mod paths;
pub mod private;
pub mod process;
pub mod programs;
pub mod runtime;
mod security;

#[cfg(feature = "platform")]
pub mod lifecycle;
