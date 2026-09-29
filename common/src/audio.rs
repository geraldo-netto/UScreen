//! T717 portable audio policy. Native adapters remain unavailable until validated.
//! No default-device changes, native calls, IO or automatic session restoration here.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

mod buffer;
mod settings;
pub use settings::{AudioController, AudioOptions, AudioSettings, AudioStatus};
mod session;
mod wire;
pub use buffer::{PcmQueue, RenderResult};
pub use session::{AudioSession, AudioState};
pub use wire::{AudioGrant, FrameReader, FrameWriter, PcmBlock, FRAME_HEADER_BYTES, HELLO_BYTES};

pub const SAMPLE_RATE: u32 = 48_000;
pub const BLOCK_FRAMES: usize = 480;
pub const MAX_SAMPLES: usize = BLOCK_FRAMES * 2;
pub const MAX_DRIFT_PPM: i32 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "platform", derive(clap::ValueEnum))]
#[repr(u8)]
pub enum Direction {
    Microphone = 1,
    Speakers = 2,
}

impl Direction {
    pub fn channels(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "platform", derive(clap::ValueEnum))]
#[repr(u8)]
pub enum Processing {
    #[default]
    Speech = 1,
    Raw = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioProfile {
    pub direction: Direction,
    pub processing: Processing,
    /// Target queued PCM in 10 ms steps; hard capacity remains 200 ms.
    pub buffer_ms: u16,
    pub background: bool,
}

impl AudioProfile {
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            processing: Processing::Speech,
            buffer_ms: 40,
            background: false,
        }
    }

    pub fn validate(self) -> Result<()> {
        ensure!(
            (20..=200).contains(&self.buffer_ms),
            "audio buffer must be 20–200 ms"
        );
        ensure!(
            self.buffer_ms.is_multiple_of(10),
            "audio buffer must use 10 ms steps"
        );
        Ok(())
    }
}

/// Advertise only native, validated capabilities for the current endpoint/route.
/// Speech means that processing can be requested, not that AEC is effective.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioCapabilities {
    pub microphone: bool,
    pub speakers: bool,
    pub speech: bool,
    pub raw: bool,
    pub background: bool,
}

impl AudioCapabilities {
    pub fn validate(self, profile: AudioProfile) -> Result<()> {
        profile.validate()?;
        let direction = match profile.direction {
            Direction::Microphone => self.microphone,
            Direction::Speakers => self.speakers,
        };
        ensure!(direction, "audio direction/48 kHz s16le format unsupported");
        let processing = match profile.processing {
            Processing::Speech => self.speech,
            Processing::Raw => self.raw,
        };
        ensure!(processing, "audio processing unsupported");
        ensure!(
            !profile.background || self.background,
            "background audio unsupported"
        );
        Ok(())
    }
}

/// Each direction owns a separate adapter/session. Calls enqueue bounded work;
/// they must not wait for native retirement on UI or real-time threads.
/// Native completion reports the grant's generation to AudioSession::retired.
/// All IO waits need cancellation/deadlines in the adapter. No defaults may change.
pub trait AudioBackend {
    fn capabilities(&self) -> AudioCapabilities;
    fn start(&mut self, grant: AudioGrant) -> Result<()>;
    fn stop(&mut self, generation: u64);
}

#[cfg(test)]
mod tests;
