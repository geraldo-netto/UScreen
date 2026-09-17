//! Shared media and live encoding settings; no capture or process dependencies.
use bytes::Bytes;
use std::sync::Arc;

/// Which bitstream syntax is in play. H.264 and HEVC agree on Annex B start
/// codes and on nothing else that matters here: the NAL header is one byte
/// against two, the type lives in different bits, and a keyframe is a
/// different set of type numbers.
///
/// Not gated on the packetizer's feature flag: the daemon has to tell the
/// tablet which codec to build a decoder for regardless of how it was
/// compiled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    H264,
    Hevc,
}

impl Codec {
    pub fn from_encoder(name: &str) -> Self {
        if name.contains("hevc") || name.contains("h265") || name.contains("265") {
            Codec::Hevc
        } else {
            Codec::H264
        }
    }

    /// Name for ffmpeg's `-f`, which wants the bitstream format, not the
    /// encoder.
    pub fn muxer(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::Hevc => "hevc",
        }
    }
}
/// One encoded access unit, tagged so the stream server can drop frames
/// safely (resume only at an IDR).
#[derive(Clone)]
pub struct VideoPacket {
    pub data: Bytes,
    pub is_idr: bool,
    /// Allocated across encoder restarts in this daemon instance. Echoed back by the tablet once
    /// the frame is on screen, measuring packet-send-to-render-ack latency.
    pub seq: u32,
    /// Immutable headers for this access unit, never the latest global cache.
    pub codec_config: Option<Bytes>,
    pub generation: Arc<std::sync::atomic::AtomicBool>,
}

/// Retires queued frames when their encoder exits or is cancelled.
pub struct EncoderGeneration {
    pub active: Arc<std::sync::atomic::AtomicBool>,
}

impl EncoderGeneration {
    pub fn new() -> Self {
        Self {
            active: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }
}

impl Drop for EncoderGeneration {
    fn drop(&mut self) {
        self.active
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Settings that can change at runtime (from the GUI or the tablet app).
/// A change restarts the encoder; an fps or resolution change also restarts
/// the helper (the EDID is regenerated for the new mode).
#[derive(Clone, Debug, PartialEq)]
pub struct EncoderSettings {
    pub encoder: String,
    pub fps: u32,
    pub bitrate: u32,
    pub width: u32,
    pub height: u32,
    /// Constant-quality target; see `config::DEFAULT_QUALITY`.
    pub quality: u32,
    /// Physical panel size for the generated EDID, in millimetres.
    pub width_mm: u32,
    pub height_mm: u32,
    /// Integer downscale for the stream; see `config::FileConfig::stream_scale`.
    pub stream_scale: u32,
    /// Authenticated tablet metadata has supplied pixel and physical geometry.
    pub geometry_ready: bool,
}

impl EncoderSettings {
    pub(crate) fn helper_geometry(&self) -> (u32, u32, u32, u32, u32, u32) {
        (
            self.width,
            self.height,
            self.width_mm,
            self.height_mm,
            self.fps,
            self.stream_scale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codec_is_picked_from_the_encoder_name() {
        assert_eq!(Codec::from_encoder("h264_nvenc"), Codec::H264);
        assert_eq!(Codec::from_encoder("libx264"), Codec::H264);
        assert_eq!(Codec::from_encoder("hevc_nvenc"), Codec::Hevc);
        assert_eq!(Codec::from_encoder("hevc_vaapi"), Codec::Hevc);
    }
}
