//! Stream configuration and geometry policy behind the control connection.
use crate::media::EncoderSettings;
use tokio::sync::watch;
use tracing::{info, warn};

/// Commands accepted from an authenticated controller. Persistence and channel
/// policy stay in the adapter, not in the wire dispatcher.
pub(super) trait SettingsSink: Sync {
    fn resolution(&self, pixels: (u32, u32), millimetres: (u32, u32));
    fn configure(&self, bitrate: Option<u32>, fps: Option<u32>, encoder: Option<String>);
    fn mode(&self, pen_only: bool);
    fn decoders(&self, capabilities: crate::media::DecoderCapabilities);
}

pub(super) struct SessionSettings<'a> {
    settings: &'a Option<watch::Sender<EncoderSettings>>,
    mode: &'a watch::Sender<bool>,
    pen_enabled: bool,
    auto_resolution: fn() -> bool,
}

impl<'a> SessionSettings<'a> {
    pub(super) fn new(
        settings: &'a Option<watch::Sender<EncoderSettings>>,
        mode: &'a watch::Sender<bool>,
        pen_enabled: bool,
    ) -> Self {
        Self {
            settings,
            mode,
            pen_enabled,
            // Preserve live persistent-policy reads on resolution messages.
            auto_resolution: || crate::config::FileConfig::load().auto_resolution,
        }
    }
}

impl SessionSettings<'_> {
    pub(super) fn sender(&self) -> Option<watch::Sender<EncoderSettings>> {
        self.settings.clone()
    }
    pub(super) fn forget_decoders(&self) {
        if let Some(tx) = self.settings {
            tx.send_modify(EncoderSettings::clear_decoders);
        }
    }
}

impl SettingsSink for SessionSettings<'_> {
    fn resolution(&self, pixels: (u32, u32), millimetres: (u32, u32)) {
        apply_tablet_resolution(self.settings, pixels, millimetres, (self.auto_resolution)());
    }
    fn configure(&self, bitrate: Option<u32>, fps: Option<u32>, encoder: Option<String>) {
        apply_tablet_config(self.settings, bitrate, fps, encoder);
    }
    fn decoders(&self, capabilities: crate::media::DecoderCapabilities) {
        let Some(tx) = self.settings else {
            return;
        };
        if capabilities.codecs.len() > 8 || capabilities.hardware.len() > 8 {
            return;
        }
        tx.send_if_modified(|settings| {
            if settings.decoders.as_ref() == Some(&capabilities) {
                return false;
            }
            settings.decoders = Some(capabilities);
            true
        });
    }
    fn mode(&self, pen_only: bool) {
        apply_tablet_mode(self.mode, pen_only, self.pen_enabled);
    }
}

pub(super) fn physical_dimensions(width: u32, height: u32) -> (u32, u32) {
    // Reject nonsense physical sizes instead of baking an absurd DPI into EDID.
    if (50..=1000).contains(&width) && (50..=1000).contains(&height) {
        (width, height)
    } else {
        (
            crate::edid::DEFAULT_WIDTH_MM,
            crate::edid::DEFAULT_HEIGHT_MM,
        )
    }
}

pub(super) fn apply_tablet_resolution(
    settings_tx: &Option<watch::Sender<EncoderSettings>>,
    pixels: (u32, u32),
    millimetres: (u32, u32),
    auto_resolution: bool,
) {
    let (width, height) = pixels;
    let (width_mm, height_mm) = millimetres;
    info!(
        "Tablet reports native resolution: {}x{} ({}x{} mm)",
        width, height, width_mm, height_mm
    );
    let Some(tx) = settings_tx else { return };
    tx.send_if_modified(|current| {
        let Some(new) = negotiated_geometry(current, pixels, millimetres, auto_resolution) else {
            return false;
        };
        replace_if_changed(current, new)
    });
}

pub(crate) fn negotiated_geometry(
    current: &EncoderSettings,
    pixels: (u32, u32),
    millimetres: (u32, u32),
    auto_resolution: bool,
) -> Option<EncoderSettings> {
    if pixels.0 == 0 || pixels.1 == 0 {
        warn!("Ignoring empty native resolution {}x{}", pixels.0, pixels.1);
        return None;
    }
    let selected = if auto_resolution {
        pixels
    } else {
        (current.width, current.height)
    };
    if !(640..=crate::config::MAX_DIMENSION).contains(&selected.0)
        || !(480..=crate::config::MAX_DIMENSION).contains(&selected.1)
    {
        warn!(
            "Ignoring unsupported capture resolution {}x{}",
            selected.0, selected.1
        );
        return None;
    }
    let mut settings = current.clone();
    (settings.width, settings.height) = selected;
    (settings.width_mm, settings.height_mm) = physical_dimensions(millimetres.0, millimetres.1);
    settings.geometry_ready = true;
    Some(settings)
}

pub(super) fn apply_tablet_config(
    settings_tx: &Option<watch::Sender<EncoderSettings>>,
    bitrate: Option<u32>,
    fps: Option<u32>,
    encoder: Option<String>,
) {
    let Some(tx) = settings_tx else {
        warn!("Received config from tablet but live settings are disabled");
        return;
    };
    // Normalize before locking: logging must not open a read/modify/write gap.
    let bitrate = bitrate.map(clamp_bitrate);
    let fps = fps.map(|f| f.clamp(crate::config::MIN_FPS, crate::config::MAX_FPS));
    let encoder = encoder.filter(|e| match crate::config::validate_encoder_for_build(e) {
        Ok(()) => true,
        Err(error) => {
            warn!("Ignoring unsupported encoder from tablet: {e}: {error}");
            false
        }
    });
    let changed = tx.send_if_modified(|current| {
        let mut new = current.clone();
        new.bitrate = bitrate.unwrap_or(current.bitrate);
        new.fps = fps.unwrap_or(current.fps);
        new.encoder = encoder.unwrap_or_else(|| current.encoder.clone());
        replace_if_changed(current, new)
    });
    if changed {
        log_settings(tx);
    }
}

fn log_settings(tx: &watch::Sender<EncoderSettings>) {
    let current = tx.borrow().clone();
    info!(
        "Live settings after tablet update: encoder={} {}kbps @{}fps",
        current.encoder, current.bitrate, current.fps
    );
}

fn clamp_bitrate(requested: u32) -> u32 {
    let clamped = requested.clamp(
        crate::config::MIN_BITRATE_KBPS,
        crate::config::MAX_BITRATE_KBPS,
    );
    if clamped != requested {
        warn!(
            "Tablet asked for {} kbps — clamped to {}",
            requested, clamped
        );
    }
    clamped
}

fn replace_if_changed(current: &mut EncoderSettings, new: EncoderSettings) -> bool {
    if *current == new {
        return false;
    }
    *current = new;
    true
}

pub(super) fn apply_tablet_mode(mode_tx: &watch::Sender<bool>, pen_only: bool, pen_enabled: bool) {
    // Only publish a real change. A watch send always wakes every
    // follower, so re-sending the current mode would tear the virtual
    // display down and back up for nothing.
    if *mode_tx.borrow() == pen_only {
        return;
    }
    // Pen-only mode with no pen device would tear the display down
    // and then drop every stroke: a blank tablet. The app's switch
    // follows the mode the daemon reports, so it simply stays off.
    if pen_only && !pen_enabled {
        warn!("Tablet asked for pen-only mode, but input_pen is off in config.toml — ignored");
        return;
    }
    info!(
        "Tablet switched to {}",
        if pen_only {
            "pen-only mode"
        } else {
            "second-screen mode"
        }
    );
    let _ = mode_tx.send(pen_only);
}
