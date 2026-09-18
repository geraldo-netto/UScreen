//! Bitstream identity is independent of its encoder and temporary CLI container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    H264,
    Hevc,
    Vp9,
    Av1,
}

impl Codec {
    pub const ALL: [Self; 4] = [Self::H264, Self::Hevc, Self::Vp9, Self::Av1];
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Hevc => "HEVC",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
        }
    }

    pub fn from_encoder(name: &str) -> Self {
        match name {
            "libaom-av1" | "av1_nvenc" | "av1_vaapi" => Self::Av1,
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
            Self::Av1 => "av1",
        }
    }
    pub fn muxer(self) -> &'static str {
        match self {
            Self::Vp9 | Self::Av1 => "ivf",
            other => other.wire_name(),
        }
    }
    pub fn framed(self) -> bool {
        matches!(self, Self::Vp9 | Self::Av1)
    }
}
