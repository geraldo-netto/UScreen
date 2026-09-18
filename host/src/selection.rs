//! Automatic selection uses short, bounded host probes and advertised decoder
//! compatibility. Actual render acknowledgements validate each selected stream.
use crate::media::{DecoderCapabilities, EncoderSettings};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Key {
    pub format: (u32, u32, u32, u32, u32),
    epoch: u64,
    decoders: Option<DecoderCapabilities>,
}
impl Key {
    #[cfg(not(feature = "inproc-encoder"))]
    pub fn new(settings: &EncoderSettings) -> Self {
        let (width, height) = settings.video_dimensions();
        Self {
            format: (
                width,
                height,
                settings.fps,
                settings.bitrate,
                settings.quality,
            ),
            epoch: settings.decoder_epoch,
            decoders: settings.decoders.clone(),
        }
    }
    pub fn matches(&self, settings: &EncoderSettings) -> bool {
        let (width, height) = settings.video_dimensions();
        settings.encoder == "auto"
            && settings.geometry_ready
            && self.format
                == (
                    width,
                    height,
                    settings.fps,
                    settings.bitrate,
                    settings.quality,
                )
            && self.epoch == settings.decoder_epoch
            && self.decoders == settings.decoders
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Selected {
    pub key: Key,
    pub encoder: String,
    pub reason: String,
    pub verified: bool,
}

#[cfg(not(feature = "inproc-encoder"))]
mod health;
#[cfg(not(feature = "inproc-encoder"))]
mod worker;
#[cfg(not(feature = "inproc-encoder"))]
pub(crate) use worker::spawn;
