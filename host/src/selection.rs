//! Linux encoder selection workers; portable selection data lives in the library.
#[cfg(not(feature = "inproc-encoder"))]
pub(crate) use uscreen::selection::{Key, Selected};

#[cfg(not(feature = "inproc-encoder"))]
mod health;
#[cfg(not(feature = "inproc-encoder"))]
mod trial;
#[cfg(not(feature = "inproc-encoder"))]
mod worker;
#[cfg(not(feature = "inproc-encoder"))]
pub(crate) use worker::spawn;
