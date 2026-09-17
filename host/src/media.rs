//! Shared media and live encoding settings; no capture or process dependencies.
use crate::media_storage::MediaBytes as Bytes;
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
/// Immutable current codec headers with race-free publication notifications.
/// T405: publishers may run on native encoder threads without a Tokio runtime.
#[derive(Clone)]
pub(crate) struct CodecConfig(tokio::sync::watch::Sender<Option<Bytes>>);

impl Default for CodecConfig {
    fn default() -> Self {
        Self::new(None)
    }
}
impl CodecConfig {
    pub fn new(value: Option<Bytes>) -> Self {
        Self(tokio::sync::watch::channel(value).0)
    }
    pub fn current(&self) -> Option<Bytes> {
        self.0.borrow().clone()
    }
    pub fn publish(&self, value: Option<Bytes>) {
        self.0.send_if_modified(|current| {
            if *current == value {
                return false;
            }
            *current = value;
            true
        });
    }
    pub async fn wait_ready(&self) -> Option<Bytes> {
        // Subscribe before examining the value. A publish between a check and
        // suspension stays observable in the watch version; no lost wakeup.
        let mut receiver = self.0.subscribe();
        let ready = receiver.wait_for(|value| value.is_some()).await.ok()?;
        ready.clone()
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
    fn t401_delayed_packet_lease_keeps_storage_after_producer_retirement() {
        use std::sync::atomic::Ordering;
        let owner = EncoderGeneration::new();
        let packet = VideoPacket {
            data: Bytes::from(vec![0, 0, 1, 0x65, 0x80]),
            is_idr: true,
            seq: u32::MAX,
            codec_config: Some(Bytes::from(vec![0, 0, 1, 0x67, 0x42])),
            generation: owner.active.clone(),
        };
        let delayed = packet.clone();
        let storage = packet.data.as_ptr();
        let configuration = packet.codec_config.as_ref().unwrap().as_ptr();
        drop(packet);
        drop(owner);
        let replacement = EncoderGeneration::new();

        // Retirement forbids sending; it must not invalidate memory held by
        // slow consumers, or substitute a replacement encoder's CSD/epoch.
        assert!(!delayed.generation.load(Ordering::Acquire));
        assert!(replacement.active.load(Ordering::Acquire));
        assert!(!Arc::ptr_eq(&delayed.generation, &replacement.active));
        assert_eq!(delayed.data.as_ptr(), storage);
        assert_eq!(delayed.data.as_ref(), &[0, 0, 1, 0x65, 0x80]);
        let config = delayed.codec_config.as_ref().unwrap();
        assert_eq!(config.as_ptr(), configuration);
        assert_eq!(config.as_ref(), &[0, 0, 1, 0x67, 0x42]);
        assert_eq!(delayed.seq, u32::MAX);
    }

    #[test]
    fn codec_is_picked_from_the_encoder_name() {
        assert_eq!(Codec::from_encoder("h264_nvenc"), Codec::H264);
        assert_eq!(Codec::from_encoder("libx264"), Codec::H264);
        assert_eq!(Codec::from_encoder("hevc_nvenc"), Codec::Hevc);
        assert_eq!(Codec::from_encoder("hevc_vaapi"), Codec::Hevc);
    }
}

#[cfg(test)]
mod codec_config_tests {
    use super::*;

    #[tokio::test]
    async fn t405_headers_published_without_waiters_survive_clone_and_reset() {
        let headers = CodecConfig::default();
        headers.publish(Some(Bytes::from(vec![1, 2, 3])));
        let next = headers.clone();
        drop(headers);
        let previous = next.wait_ready().await.unwrap();
        next.publish(None);
        assert!(next.current().is_none());
        next.publish(Some(Bytes::from(vec![4, 5])));
        assert_eq!(next.wait_ready().await.unwrap().as_ref(), &[4, 5]);
        assert_eq!(previous.as_ref(), &[1, 2, 3]);
    }
}
