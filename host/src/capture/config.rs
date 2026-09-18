use std::path::PathBuf;

#[derive(Clone)]
pub struct CaptureConfig {
    pub helper_path: PathBuf,
    /// Explicit EDID override; None = generate one for the configured mode
    pub edid_path: Option<PathBuf>,
    pub encoder: String,
    /// Control-side decoder request whose ACKs may certify this encoder generation.
    pub decoder: Option<uscreen_config::negotiation::DecoderChoice>,
    // The experimental in-process encoder does not create VAAPI contexts.
    #[cfg_attr(feature = "inproc-encoder", allow(dead_code))]
    pub vaapi_device: String,
    pub fps: u32,
    pub bitrate: u32,
    pub width: u32,
    pub height: u32,
    pub quality: u32,
    pub width_mm: u32,
    pub height_mm: u32,
    pub stream_scale: u32,
    /// Maximum Linux conversion participants including the caller; 0 = Auto.
    pub conversion_threads: u32,
    /// Which edge of the existing desktop the virtual screen sits against.
    /// Not an encoder setting: changing it moves a window, it does not
    /// restart a stream.
    pub position: crate::config::Position,
    pub ten_bit: bool,
    /// Which tablet this pipeline serves (0 = the first): picks the FIFO
    /// name. Exclusive helper leases keep active pipelines on different cards.
    pub instance: u32,
    /// An explicit strict card pin. Automatic sessions leave this unset and
    /// acquire a free-card lease, preferring their previous card on restart.
    pub card: Option<u32>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            helper_path: PathBuf::from("host/evdi/evdi_helper"),
            edid_path: None,
            encoder: String::from("h264_nvenc"),
            decoder: None,
            vaapi_device: "/dev/dri/renderD128".into(),
            fps: 60,
            bitrate: 20000,
            width: 2960,
            height: 1848,
            quality: crate::config::DEFAULT_QUALITY,
            width_mm: crate::edid::DEFAULT_WIDTH_MM,
            height_mm: crate::edid::DEFAULT_HEIGHT_MM,
            stream_scale: 1,
            conversion_threads: 0,
            position: crate::config::Position::Right,
            ten_bit: false,
            instance: 0,
            card: None,
        }
    }
}
