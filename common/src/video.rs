//! Bitstream identity is independent of its encoder and temporary CLI container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    H264,
    Hevc,
    Vp9,
}

impl Codec {
    pub fn from_encoder(name: &str) -> Self {
        match name {
            "libvpx-vp9" | "vp9_vaapi" => Self::Vp9,
            name if name.contains("hevc") || name.contains("265") => Self::Hevc,
            _ => Self::H264,
        }
    }
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Vp9 => "vp9",
        }
    }
    pub fn muxer(self) -> &'static str {
        match self {
            Self::Vp9 => "ivf",
            other => other.wire_name(),
        }
    }
    pub fn framed(self) -> bool {
        self == Self::Vp9
    }
}
