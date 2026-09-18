//! Platform-neutral, bounded media advertisements. Support is not a benchmark.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct DecoderCapabilities {
    pub protocol: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codecs: Vec<String>,
    #[serde(default)]
    pub hardware: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<Decoder>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Decoder {
    pub name: String,
    pub codec: String,
    pub hardware: Option<bool>,
    pub low_latency: Option<bool>,
    /// Supported rate at this report's exact width and height, not throughput.
    pub operating_rate: Option<u32>,
    pub profiles: Vec<Profile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Profile {
    pub profile: String,
    /// Codec-standard level times ten; HEVC main tier only in this revision.
    pub level: u32,
    pub depth: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct StreamProfile {
    pub codec: String,
    #[serde(flatten)]
    pub format: Profile,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct DecoderChoice {
    pub name: String,
    pub stream: StreamProfile,
    /// Only standard hints supported for this decoder and format may be requested.
    pub low_latency: bool,
    pub operating_rate: Option<u32>,
}

impl DecoderChoice {
    /// Bounded configuration receipt, not authentication or proof a hint was honored.
    /// Length-prefix the only arbitrary field so delimiter-containing names cannot collide.
    pub fn receipt(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}",
            self.name.len(),
            self.name,
            self.stream.codec,
            self.stream.format.profile,
            self.stream.format.level,
            self.stream.format.depth,
            u8::from(self.low_latency),
            self.operating_rate.unwrap_or(0)
        )
    }
}

impl DecoderCapabilities {
    pub fn valid(&self) -> bool {
        let bounded = (2..=4096).contains(&self.width)
            && (2..=4096).contains(&self.height)
            && (10..=90).contains(&self.fps);
        bounded && self.valid_families() && self.valid_extension()
    }

    fn valid_families(&self) -> bool {
        self.codecs.len() <= 4
            && self.hardware.len() <= 4
            && self.codecs.iter().all(|c| family(c))
            && self.hardware.iter().all(|c| self.codecs.contains(c))
    }

    fn valid_extension(&self) -> bool {
        match self.protocol {
            1 => self.scope.is_none() && self.details.is_empty(),
            2 => self.valid_details(),
            _ => false,
        }
    }

    fn valid_details(&self) -> bool {
        self.scope.as_ref().is_some_and(|s| identifier(s, 96))
            && self.details.len() <= 16
            && self.details.iter().all(|d| d.valid(&self.codecs))
            && unique_decoders(&self.details)
    }

    pub fn matches(&self, scope: &str, width: u32, height: u32, fps: u32) -> bool {
        self.valid()
            && (self.width, self.height, self.fps) == (width, height, fps)
            && (self.protocol == 1 || self.scope.as_deref() == Some(scope))
    }

    /// Rich support requires an actual encoder profile; unknown does not grant it.
    pub fn choose(&self, stream: &StreamProfile, standard_hints: bool) -> Option<DecoderChoice> {
        self.choices(stream, standard_hints).into_iter().next()
    }

    /// Compatible alternatives in platform order, with hardware advertised first.
    pub fn choices(&self, stream: &StreamProfile, standard_hints: bool) -> Vec<DecoderChoice> {
        if self.protocol != 2 || !self.valid() || !stream.valid() {
            return Vec::new();
        }
        let mut decoders: Vec<_> = self.details.iter().filter(|d| d.supports(stream)).collect();
        decoders.sort_by_key(|d| d.hardware != Some(true));
        decoders
            .into_iter()
            .map(|decoder| DecoderChoice {
                name: decoder.name.clone(),
                stream: stream.clone(),
                low_latency: standard_hints && decoder.low_latency == Some(true),
                operating_rate: decoder.operating_rate.filter(|_| standard_hints),
            })
            .collect()
    }
}

impl Decoder {
    fn valid(&self, families: &[String]) -> bool {
        identifier(&self.name, 128)
            && families.contains(&self.codec)
            && self.profiles.len() <= 32
            && self.profiles.iter().all(|p| p.valid(&self.codec))
            && self
                .operating_rate
                .is_none_or(|rate| (10..=180).contains(&rate))
    }

    fn supports(&self, stream: &StreamProfile) -> bool {
        self.codec == stream.codec && self.profiles.iter().any(|p| p.covers(&stream.format))
    }
}

impl StreamProfile {
    pub fn valid(&self) -> bool {
        self.format.valid(&self.codec)
    }
}

impl Profile {
    fn valid(&self, codec: &str) -> bool {
        let known = matches!(
            (codec, self.profile.as_str(), self.depth),
            (
                "h264",
                "baseline" | "constrained-baseline" | "main" | "high",
                8
            ) | ("hevc", "main", 8)
                | ("hevc", "main10", 10)
                | ("vp9", "profile0", 8)
                | ("vp9", "profile2", 10)
                | ("av1", "main", 8 | 10)
        );
        known && level_valid(codec, self.level)
    }

    fn covers(&self, required: &Self) -> bool {
        let profile = self.profile == required.profile
            || (self.profile == "baseline" && required.profile == "constrained-baseline");
        // AVC level 1b (9) is above level 1 (10), below level 1.1 (11).
        let order = |level| if level == 9 { 105 } else { level * 10 };
        profile && self.depth == required.depth && order(self.level) >= order(required.level)
    }
}

fn level_valid(codec: &str, level: u32) -> bool {
    let levels: &[u32] = match codec {
        "h264" => &[
            9, 10, 11, 12, 13, 20, 21, 22, 30, 31, 32, 40, 41, 42, 50, 51, 52, 60, 61, 62,
        ],
        "hevc" => &[10, 20, 21, 30, 31, 40, 41, 50, 51, 52, 60, 61, 62],
        "vp9" => &[10, 11, 20, 21, 30, 31, 40, 41, 50, 51, 52, 60, 61, 62],
        "av1" => &[
            20, 21, 22, 23, 30, 31, 32, 33, 40, 41, 42, 43, 50, 51, 52, 53, 60, 61, 62, 63, 70, 71,
            72, 73,
        ],
        _ => &[],
    };
    levels.contains(&level)
}

fn family(value: &str) -> bool {
    matches!(value, "h264" | "hevc" | "vp9" | "av1")
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|b| b.is_ascii_graphic())
}

fn unique_decoders(decoders: &[Decoder]) -> bool {
    decoders.iter().enumerate().all(|(i, d)| {
        !decoders[..i].iter().any(|other| {
            (other.name.as_str(), other.codec.as_str()) == (d.name.as_str(), d.codec.as_str())
        })
    })
}

#[cfg(test)]
mod tests;
