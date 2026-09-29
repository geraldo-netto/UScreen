use super::{AudioProfile, AudioState, Direction};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioOptions {
    pub profile: AudioProfile,
    pub serial: Option<String>,
}
impl AudioOptions {
    pub fn new(direction: Direction) -> Self {
        Self {
            profile: AudioProfile::new(direction),
            serial: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioSettings {
    pub microphone: AudioOptions,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            microphone: AudioOptions::new(Direction::Microphone),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioStatus {
    pub state: AudioState,
    pub detail: String,
}
impl Default for AudioStatus {
    fn default() -> Self {
        Self {
            state: AudioState::Stopped,
            detail: "Stopped".into(),
        }
    }
}

/// UI-facing manual controller. Native construction belongs in one platform factory.
pub trait AudioController {
    fn start(&self, options: AudioOptions) -> Result<()>;
    fn stop(&self);
    fn status(&self) -> AudioStatus;
}
