//! Portable settings schema, validation and edit merging. No OS services.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Maximum accepted configured bitrate, in kbps. This is a project policy
/// limit, not a measured capacity for every USB link. VAAPI CQP does not
/// enforce a bitrate ceiling (T259).
pub const MAX_BITRATE_KBPS: u32 = 60_000;
pub const MIN_BITRATE_KBPS: u32 = 1_000;

/// Maximum accepted frame-rate target. EDID pixel-clock validity also depends
/// on resolution; not every combination up to this limit is valid (T332).
pub const MAX_FPS: u32 = 90;
pub const MIN_FPS: u32 = 10;

/// Constant-quality target for the encoder (lower = sharper, more bits).
///
/// The default is 18. Visual quality and bandwidth depend on encoder, content
/// and device; this value does not establish a universal quality threshold.
pub const DEFAULT_QUALITY: u32 = 18;
pub const MIN_QUALITY: u32 = 12;
pub const MAX_QUALITY: u32 = 32;

/// Maximum active pixels representable in an EDID detailed timing.
pub const MAX_DIMENSION: u32 = 4095;

/// Validate every listener before allocating any tablet slot.
pub fn slot_ports(video: u16, input: u16, slots: u32) -> Result<Vec<(u16, u16)>> {
    anyhow::ensure!(
        (1..=4).contains(&slots),
        "tablet count must be between 1 and 4"
    );
    let mut used = std::collections::BTreeSet::new();
    let mut ports = Vec::new();
    for slot in 0..slots {
        let offset = (2 * slot) as u16;
        let video = video
            .checked_add(offset)
            .context("video slot port exceeds 65535")?;
        let input = input
            .checked_add(offset)
            .context("input slot port exceeds 65535")?;
        for port in [video, input] {
            anyhow::ensure!(port != 0, "video and input ports must be nonzero");
            anyhow::ensure!(used.insert(port), "tablet ports overlap at {port}");
        }
        ports.push((video, input));
    }
    Ok(ports)
}

/// Resolve selected profiles and the legacy alias to the stock FFmpeg name.
pub fn ffmpeg_encoder_name(name: &str) -> &str {
    crate::encoding::ffmpeg_name(name)
}

pub fn supported_encoder(name: &str) -> bool {
    name == "auto" || crate::encoding::find(name).is_some()
}

pub const PIPE_CAPACITIES_MIB: [u32; 4] = [1, 2, 4, 8];

pub fn validated_pipe_capacity(value: u32) -> u32 {
    if PIPE_CAPACITIES_MIB.contains(&value) {
        value
    } else {
        1
    }
}

/// Persistent settings, shared by the CLI daemon, the GUI and the tablet app
/// (which pushes changes over the input WebSocket).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct FileConfig {
    pub encoder: String,
    /// DRM render node used by VA-API encoders.
    pub vaapi_device: String,
    pub fps: u32,
    /// kbps
    pub bitrate: u32,
    pub width: u32,
    pub height: u32,
    /// Encoder constant-quality target: lower is sharper and costs more bits.
    ///
    /// Maps to NVENC CQ, VAAPI QP or x264 CRF. NVENC/x264 also use bitrate
    /// limits; VAAPI CQP does not enforce the configured ceiling (T259).
    pub quality: u32,
    /// Integer downscale for the stream only; the desktop keeps its native
    /// mode. 1 = native, 2 = half in each axis. Fewer streamed pixels can
    /// reduce decoder work at the cost of sharpness; the effect depends on
    /// the tablet. Scaling happens after capture, so it does not shrink the grab.
    pub stream_scale: u32,
    /// Linux raw capture pipe request, per active tablet. Kernel limits may reduce it.
    pub pipe_capacity_mib: u32,
    /// Use the tablet as a graphics tablet for the laptop's own screen rather
    /// than as a second display: no capture, no encoding, nothing streamed —
    /// the pen and touch simply drive the screen you are already looking at.
    /// This removes the streamed-video path; input and host-display latency remain.
    pub pen_only: bool,
    /// Where the virtual screen sits relative to the physical ones:
    /// "right" (default), "left", "above" or "below". Anything else is
    /// treated as "right" rather than refusing to start over a typo.
    pub position: String,
    /// Encode in 10 bits (HEVC Main10). The captured desktop is 8-bit and
    /// cannot be otherwise, so this adds no colour — it gives the encoder
    /// more precision to work in, which is what removes banding from
    /// gradients at a given bitrate. Costs a format conversion per frame.
    pub ten_bit: bool,
    /// Require the tablet to present this run's session token before it is
    /// sent any video or allowed to inject input. The token is handed to the
    /// app over adb when it is launched. Without this, any local process —
    /// or any other app on the tablet — could connect to the loopback ports,
    /// read the screen and drive the mouse. Off only if you need an app
    /// older than 1.1.0 to keep working.
    pub require_token: bool,
    /// Ask GitHub once a day whether a newer release exists, and say so in
    /// the tray and in `uscreen doctor`. Nothing is ever downloaded or
    /// installed by the daemon; updating stays with you or your package
    /// manager. One HTTPS request a day to api.github.com.
    pub check_updates: bool,
    /// How many tablets may be attached at once, each as its own virtual
    /// screen. Installers prepare two EVDI devices by default; GUI system
    /// setup provisions the configured count now and at subsequent boots.
    pub max_tablets: u32,
    /// The tablet's address on the network, as `ip:port`, remembered by
    /// `uscreen wifi`. When the cable is not plugged in the daemon tries to
    /// reconnect to it by itself, so the tablet comes back as a screen
    /// without anyone typing an adb command. Empty disables that.
    pub wifi_address: String,
    /// Match the virtual display to whatever resolution the tablet reports
    pub auto_resolution: bool,
    pub video_port: u16,
    pub input_port: u16,
    /// Launch the UScreen app on the tablet automatically when it's plugged in
    pub auto_launch_app: bool,
    /// Create the virtual touchscreen ("UScreen Touch") while a tablet is
    /// attached, so taps on it become touch input here. Opt out if merely
    /// having a touchscreen upsets your desktop (Cinnamon/GNOME on X11 hide
    /// the mouse cursor around touch devices) and you only use the pen.
    pub input_touch: bool,
    /// Create the virtual pen tablet ("UScreen Pen") for stylus input with
    /// pressure, tilt and eraser. Pen-only mode needs it.
    pub input_pen: bool,
    /// Create the absolute pointer ("UScreen Pointer") that parks the mouse
    /// cursor where the pen last was, so it does not vanish when the pen
    /// lifts. Only exists together with `input_pen`.
    pub input_pointer: bool,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            encoder: "auto".into(),
            vaapi_device: "/dev/dri/renderD128".into(),
            fps: 60,
            bitrate: 20000,
            width: 2960,
            height: 1848,
            quality: DEFAULT_QUALITY,
            stream_scale: 1,
            pipe_capacity_mib: 1,
            pen_only: false,
            position: "right".into(),
            ten_bit: false,
            require_token: true,
            check_updates: true,
            max_tablets: 1,
            wifi_address: String::new(),
            auto_resolution: true,
            video_port: 8890,
            input_port: 8891,
            auto_launch_app: true,
            input_touch: true,
            input_pen: true,
            input_pointer: true,
        }
    }
}

/// Where the virtual screen goes relative to everything already on the desktop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Position {
    Right,
    Left,
    Above,
    Below,
}

impl Position {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
            Self::Above => "above",
            Self::Below => "below",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "right" => Some(Position::Right),
            "left" => Some(Position::Left),
            "above" | "top" => Some(Position::Above),
            "below" | "bottom" => Some(Position::Below),
            _ => None,
        }
    }

    /// Unknown values are a typo in a config file, not a reason to refuse to
    /// bring the screen up at all.
    pub fn parse_or_default(s: &str) -> Self {
        Self::parse(s).unwrap_or(Position::Right)
    }
}

impl FileConfig {
    pub fn validate(&self) -> Result<()> {
        self.validate_input_mode()?;
        crate::display::pixel_clock_10khz(self.width, self.height, self.fps)?;
        slot_ports(self.video_port, self.input_port, self.max_tablets)?;
        Ok(())
    }

    pub fn validate_input_mode(&self) -> Result<()> {
        anyhow::ensure!(
            !self.pen_only || self.input_pen,
            "Graphics-tablet mode requires Pen. Enable Pen or use second-screen mode."
        );
        Ok(())
    }
    /// Pipe-only edits are consumed by the existing helper at frame boundaries.
    pub fn requires_restart_from(&self, previous: &Self) -> bool {
        let mut without_pipe_edit = self.clone();
        without_pipe_edit.pipe_capacity_mib = previous.pipe_capacity_mib;
        without_pipe_edit != *previous
    }

    /// Merge only fields edited since the GUI opened into the latest disk
    /// snapshot. Future schema fields participate without a second field list.
    pub fn merge_edits(&self, baseline: &Self, latest: Self) -> Result<Self> {
        let edited = toml::Value::try_from(self)?;
        let baseline = toml::Value::try_from(baseline)?;
        let mut merged = toml::Value::try_from(latest)?;
        let table = merged
            .as_table_mut()
            .context("config must be a TOML table")?;
        for (key, value) in edited.as_table().context("config must be a TOML table")? {
            if baseline.get(key) != Some(value) {
                table.insert(key.clone(), value.clone());
            }
        }
        let mut config: Self = merged.try_into()?;
        config.sanitize();
        Ok(config)
    }

    /// Pull persisted values back into the range the pipeline can actually
    /// serve. Older builds let the tablet push 200 Mbps @ 90 fps and wrote it
    /// straight to disk, so existing installs carry settings that guarantee
    /// multi-second latency until they are clamped here.
    pub fn sanitize(&mut self) {
        if !supported_encoder(&self.encoder) {
            tracing::warn!("Unknown encoder {:?} — using auto", self.encoder);
            self.encoder = "auto".into();
        }
        let bitrate = self.bitrate.clamp(MIN_BITRATE_KBPS, MAX_BITRATE_KBPS);
        if bitrate != self.bitrate {
            tracing::warn!(
                "Bitrate {} kbps is beyond what the USB transport sustains — clamped to {} kbps",
                self.bitrate,
                bitrate
            );
            self.bitrate = bitrate;
        }

        let fps = self.fps.clamp(MIN_FPS, MAX_FPS);
        if fps != self.fps {
            tracing::warn!("fps {} out of range — clamped to {}", self.fps, fps);
            self.fps = fps;
        }

        self.quality = self.quality.clamp(MIN_QUALITY, MAX_QUALITY);
        self.stream_scale = self.stream_scale.clamp(1, 4);
        self.pipe_capacity_mib = validated_pipe_capacity(self.pipe_capacity_mib);
        self.max_tablets = self.max_tablets.clamp(1, 4);
        self.width = self.width.clamp(640, MAX_DIMENSION);
        self.height = self.height.clamp(480, MAX_DIMENSION);

        if Position::parse(&self.position).is_none() {
            tracing::warn!(
                "Unknown position {:?} — falling back to \"right\"",
                self.position
            );
            self.position = "right".into();
        }
        self.position = Position::parse_or_default(&self.position).as_str().into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t415_pipe_capacity_round_trips_and_invalid_values_fall_back() {
        for (value, expected) in [(1, 1), (2, 2), (4, 4), (8, 8), (0, 1), (3, 1), (16, 1)] {
            let mut config: FileConfig =
                toml::from_str(&format!("pipe_capacity_mib = {value}")).unwrap();
            config.sanitize();
            let saved = toml::Value::try_from(config).unwrap();
            assert_eq!(
                saved
                    .get("pipe_capacity_mib")
                    .and_then(toml::Value::as_integer),
                Some(expected),
                "T415: {value}"
            );
        }
        let defaults = toml::Value::try_from(FileConfig::default()).unwrap();
        assert_eq!(defaults["pipe_capacity_mib"].as_integer(), Some(1));
    }

    #[test]
    fn t434_auto_is_default_but_explicit_choices_survive() {
        assert!(supported_encoder("auto"));
        assert_eq!(FileConfig::default().encoder, "auto");
        for name in ["auto", "h264_vaapi_baseline", "libvpx-vp9", "libaom-av1"] {
            let mut settings = FileConfig {
                encoder: name.into(),
                ..Default::default()
            };
            settings.sanitize();
            assert_eq!(settings.encoder, name);
        }
    }

    #[test]
    fn t093_all_slot_ports_are_unique_nonzero_and_representable() {
        for (video, input, slots) in [
            (0, 8891, 1),
            (8890, 0, 1),
            (8890, 8890, 1),
            (8890, 8892, 2),
            (8890, 8894, 3),
            (65535, 65534, 2),
            (65529, 65528, 5),
        ] {
            assert!(
                slot_ports(video, input, slots).is_err(),
                "accepted {video}/{input} x{slots}"
            );
        }
        for slots in 1..=4 {
            let ports = slot_ports(8890, 8891, slots).unwrap();
            assert_eq!(ports.len(), slots as usize);
            assert_eq!(
                ports.last(),
                Some(&(8890 + 2 * (slots - 1) as u16, 8891 + 2 * (slots - 1) as u16))
            );
        }
        assert_eq!(
            slot_ports(65529, 65528, 4).unwrap().last(),
            Some(&(65535, 65534))
        );
        assert_eq!(
            slot_ports(19000, 20000, 2).unwrap(),
            [(19000, 20000), (19002, 20002)]
        );
    }

    #[test]
    fn t017_merge_preserves_external_changes_and_applies_only_user_edits() {
        let baseline = FileConfig::default();
        let edited = FileConfig {
            fps: 90,
            input_touch: false,
            ..baseline.clone()
        };
        let latest = FileConfig {
            wifi_address: "192.0.2.1:5555".into(),
            quality: 24,
            width: 1920,
            encoder: "hevc_nvenc".into(),
            fps: 30,
            ..baseline.clone()
        };
        let merged = edited.merge_edits(&baseline, latest.clone()).unwrap();
        assert_eq!(merged.wifi_address, latest.wifi_address);
        assert_eq!(merged.quality, 24);
        assert_eq!(merged.width, 1920);
        assert_eq!(merged.encoder, "hevc_nvenc");
        assert_eq!(merged.fps, 90);
        assert!(!merged.input_touch);
        assert_eq!(
            baseline.merge_edits(&baseline, latest.clone()).unwrap(),
            latest
        );
    }

    #[test]
    fn t061_dimensions_fit_edid_active_pixel_fields() {
        let mut config = FileConfig {
            width: 8192,
            height: 8192,
            ..FileConfig::default()
        };
        config.sanitize();
        assert_eq!((config.width, config.height), (4095, 4095));
    }

    #[test]
    fn t063_unknown_encoder_is_not_persisted_as_valid_configuration() {
        let mut config = FileConfig {
            encoder: "unknown-codec".into(),
            ..FileConfig::default()
        };
        config.sanitize();
        assert_eq!(config.encoder, FileConfig::default().encoder);
    }
}
