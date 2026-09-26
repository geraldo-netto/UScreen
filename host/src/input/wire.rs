//! Shared control-wire types; no device or persistence operations.
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
#[serde(tag = "type")]
pub enum InputEvent {
    #[serde(rename = "touch")]
    Touch {
        x: f64,
        y: f64,
        pressure: f64,
        action: u8,
        slot: u8,
    },
    #[serde(rename = "pen")]
    Pen {
        x: f64,
        y: f64,
        pressure: f64,
        tilt_x: f64,
        tilt_y: f64,
        /// True when the pen's eraser end is in use (TOOL_TYPE_ERASER).
        /// Emitted as BTN_TOOL_RUBBER so GIMP's eraser works.
        #[serde(default)]
        eraser: bool,
        /// Current primary stylus-button state on positional samples. Legacy
        /// clients omit this and retain explicit action 5/6 behavior.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        button: Option<bool>,
        /// 0=down 1=up 2=move 3=hover 4=hover_exit
        /// 5=stylus button down 6=stylus button up
        action: u8,
    },
    #[serde(rename = "resolution")]
    Resolution {
        width: u32,
        height: u32,
        /// Physical panel size, when the tablet knows it. Feeds the EDID so
        /// the desktop derives the right DPI and default scale.
        #[serde(default)]
        width_mm: u32,
        #[serde(default)]
        height_mm: u32,
    },
    /// Settings pushed from the tablet app's settings UI
    #[serde(rename = "config")]
    Config {
        bitrate: Option<u32>,
        fps: Option<u32>,
        encoder: Option<String>,
    },
    /// The tablet asking to switch between being a second screen and being a
    /// graphics tablet. Applied live: the host remaps the input devices onto
    /// the other output and brings the virtual display up or down to match.
    #[serde(rename = "decoders")]
    Decoders {
        capabilities: crate::media::DecoderCapabilities,
    },
    #[serde(rename = "mode")]
    Mode { pen_only: bool },
    /// Must be the first message on the socket. Proves the client is the
    /// tablet this daemon launched, not some other process on the loopback.
    #[serde(rename = "auth")]
    Auth { token: String },
    /// Android executed a render callback for this frame. The host times
    /// encoded-packet readiness through ACK receipt on its own clock;
    /// neither event proves optical presentation or requires clock agreement.
    #[serde(rename = "rendered")]
    Rendered {
        seq: u32,
        /// Complete-frame arrival to render-callback execution in microseconds
        /// on the tablet clock. Independent host/tablet percentiles cannot be
        /// subtracted to obtain transport latency.
        #[serde(default)]
        decode_us: i64,
        /// Configuration receipt from the decoder which actually rendered this sequence.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decoder: Option<String>,
    },
}

#[derive(Serialize)]
pub struct InputResponse {
    pub status: String,
    /// Accepted ADB route, independent of loopback addresses and charging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    pub width: u32,
    pub height: u32,
    pub video_width: u32,
    pub video_height: u32,
    /// Effective bitstream identity. Framed codecs additionally validate the
    /// versioned video configuration against this control announcement.
    pub codec: String,
    pub requested_encoder: String,
    pub effective_encoder: String,
    pub selection_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoder_protocol: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoder_scope: Option<String>,
    pub decoder_selection: Option<blent_config::negotiation::DecoderChoice>,
    /// Tells the tablet not to expect a video stream: it is acting as a
    /// graphics tablet for the host's own screen, not as a display.
    pub pen_only: bool,
    pub touch: bool,
    pub pen: bool,
}
