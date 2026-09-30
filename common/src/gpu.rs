//! Portable encoding-device choices. Native adapters own discovery and handles.
use crate::encoding::{find, Backend};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub id: String,
    pub label: String,
    pub backend: Backend,
    /// Device access only; encoder/format support still needs a real encode probe.
    pub accessible: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub supported: bool,
    pub adapters: Vec<Adapter>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Policy {
    #[default]
    Automatic,
    Pinned(Backend),
    Software,
}

impl Catalog {
    pub fn requested(&self, id: &str) -> String {
        match id {
            "" => "Automatic / existing device setting".into(),
            "software" => "CPU (software encoders)".into(),
            _ => self
                .adapters
                .iter()
                .find(|adapter| adapter.id == id)
                .map(|adapter| adapter.label.clone())
                .unwrap_or_else(|| {
                    format!(
                        "Unavailable saved GPU: {}",
                        id.chars().take(100).collect::<String>()
                    )
                }),
        }
    }
    pub fn policy(&self, id: &str) -> Policy {
        if id.is_empty() {
            return Policy::Automatic;
        }
        self.adapters
            .iter()
            .find(|adapter| self.supported && adapter.id == id && adapter.accessible)
            .map(|adapter| Policy::Pinned(adapter.backend))
            .unwrap_or(Policy::Software)
    }
}

impl Policy {
    pub fn allows(self, name: &str) -> bool {
        if self == Self::Automatic || name == "auto" {
            return true;
        }
        let Some(encoder) = find(name) else {
            return false;
        };
        if matches!(encoder.backend, Backend::X264 | Backend::Vpx | Backend::Aom) {
            return true;
        }
        self == Self::Pinned(encoder.backend)
    }
    pub fn encoder<'a>(self, requested: &'a str) -> &'a str {
        if self.allows(requested) {
            requested
        } else {
            "libx264"
        }
    }
}

#[cfg(test)]
mod tests;
