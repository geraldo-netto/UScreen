//! Control endpoint configuration and response snapshots.
use super::wire::InputResponse;
use crate::media::EncoderSettings;
use tokio::sync::watch;

#[derive(Clone)]
pub struct InputConfig {
    pub port: u16,
    /// Which tablet this server belongs to (0 = the first). Decides device
    /// names, product ids, and which virtual output the devices map to.
    pub instance: u32,
    /// Session token the client must present first; `None` disables it.
    pub token: Option<String>,
    /// Bitstream the encoder produces, so connecting clients can be told.
    pub codec: String,
    pub virtual_width: u32,
    pub virtual_height: u32,
    /// Which virtual input devices to create while a tablet is attached. A
    /// device that is off is never registered with the kernel: the desktop
    /// does not see it, and input of that kind from the tablet is dropped
    /// (logged at debug level). The pointer needs the pen.
    pub touch: bool,
    pub pen: bool,
    pub pointer: bool,
}

impl InputConfig {
    pub(super) fn response(
        &self,
        status: &str,
        pen_only: bool,
        settings: &Option<watch::Sender<EncoderSettings>>,
    ) -> InputResponse {
        // Keep one read guard: codec and frame rate must describe the same
        // settings revision even while another controller applies a change.
        let settings = settings.as_ref().map(|tx| tx.borrow());
        let codec = settings
            .as_ref()
            .map(|current| {
                crate::media::Codec::from_encoder(&current.encoder)
                    .muxer()
                    .to_string()
            })
            .unwrap_or_else(|| self.codec.clone());
        InputResponse {
            status: status.into(),
            transport: None,
            fps: settings.as_ref().map(|current| current.fps),
            width: self.virtual_width,
            height: self.virtual_height,
            codec,
            pen_only,
            touch: self.touch,
            pen: self.pen,
        }
    }

    pub fn any_device(&self) -> bool {
        self.touch || self.pen
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            port: 8891,
            instance: 0,
            token: None,
            codec: "h264".into(),
            virtual_width: 2960,
            virtual_height: 1848,
            touch: true,
            pen: true,
            pointer: true,
        }
    }
}
