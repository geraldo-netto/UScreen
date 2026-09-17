pub mod commands;
pub mod runtime;
pub mod version;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Highest bitrate the USB transport actually sustains. Beyond this the encoder
/// outruns the link, frames pile up in every queue along the way and latency
/// grows without bound — the stream does not get sharper, only later.
///
/// This is a hard ceiling rather than a hint because the value is persisted:
/// a bad number pushed once from the tablet stays in the config file and keeps
/// poisoning every subsequent run.
pub const MAX_BITRATE_KBPS: u32 = 60_000;
pub const MIN_BITRATE_KBPS: u32 = 1_000;

/// The generated EDID caps the virtual mode at 90 Hz (EDID 1.4 stores the pixel
/// clock in 16 bits, and 2960x1848@120 overflows it), so anything above 90
/// would only ever produce duplicate frames.
pub const MAX_FPS: u32 = 90;
pub const MIN_FPS: u32 = 10;

/// Constant-quality target for the encoder (lower = sharper, more bits).
///
/// 18 rather than a more conservative value because bandwidth stopped being the
/// constraint: in constant-quality mode a desktop streams at a few Mbps against
/// a ceiling tens of times higher, so spending bits on crisp text is close to
/// free. Text sharpness is ultimately limited by 4:2:0 chroma subsampling, not
/// by this number — below roughly 16 there is nothing left to gain.
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

/// Preserve the legacy GStreamer-style setting while using FFmpeg's name.
pub fn ffmpeg_encoder_name(name: &str) -> &str {
    match name {
        "vaapih264enc" => "h264_vaapi",
        _ => name,
    }
}

pub fn supported_encoder(name: &str) -> bool {
    matches!(
        name,
        "h264_nvenc" | "hevc_nvenc" | "h264_vaapi" | "hevc_vaapi" | "libx264" | "vaapih264enc"
    )
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
    /// This, not the bitrate, is what governs picture quality now that the
    /// encoder runs in constant-quality mode — the bitrate is only a ceiling
    /// for bursts, and on a desktop the stream sits far below it.
    pub quality: u32,
    /// Integer downscale for the stream only; the desktop keeps its native
    /// mode. 1 = native, 2 = half. Trades sharpness for latency: the tablet's
    /// decoder costs roughly 7-8ms fixed plus 1.2ms per megapixel, so fewer
    /// pixels arrive on screen sooner.
    pub stream_scale: u32,
    /// Use the tablet as a graphics tablet for the laptop's own screen rather
    /// than as a second display: no capture, no encoding, nothing streamed —
    /// the pen and touch simply drive the screen you are already looking at.
    /// For drawing that removes display latency from the loop entirely, which
    /// is worth more than any amount of tuning the video path.
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
            encoder: "h264_nvenc".into(),
            vaapi_device: "/dev/dri/renderD128".into(),
            fps: 60,
            bitrate: 20000,
            width: 2960,
            height: 1848,
            quality: DEFAULT_QUALITY,
            stream_scale: 1,
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

pub fn daemon_is_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let base = PathBuf::from(format!("/proc/{pid}"));
    if std::fs::read_to_string(base.join("comm")).map_or(true, |name| name.trim() != "uscreen") {
        return false;
    }
    // A zombie has finished cleanup but may not yet have been reaped by its
    // parent. Waiting for /proc to disappear would misreport it as running.
    std::fs::read_to_string(base.join("stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, fields)| !fields.starts_with('Z') && !fields.starts_with('X'))
        })
        .unwrap_or(false)
}

pub fn spawn_reaped(command: &mut std::process::Command) -> std::io::Result<u32> {
    let mut child = command.spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("uscreen/config.toml")
}

impl FileConfig {
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

    pub fn load() -> Self {
        Self::load_at(&config_path())
    }

    pub fn load_at(path: &Path) -> Self {
        let mut cfg = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("Invalid config at {:?}: {} — using defaults", path, e);
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        cfg.sanitize();
        cfg
    }

    /// Pull persisted values back into the range the pipeline can actually
    /// serve. Older builds let the tablet push 200 Mbps @ 90 fps and wrote it
    /// straight to disk, so existing installs carry settings that guarantee
    /// multi-second latency until they are clamped here.
    pub fn sanitize(&mut self) {
        if !supported_encoder(&self.encoder) {
            tracing::warn!("Unknown encoder {:?} — using h264_nvenc", self.encoder);
            self.encoder = "h264_nvenc".into();
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
    }

    pub fn save(&self) -> Result<()> {
        self.save_at(&config_path())
    }

    /// Serialize the entire read/modify/write operation across processes.
    pub fn update(edit: impl FnOnce(&mut Self) -> Result<()>) -> Result<Self> {
        Self::update_at(&config_path(), edit)
    }

    pub fn save_edits(&self, baseline: &Self) -> Result<Self> {
        Self::update(|latest| {
            *latest = self.merge_edits(baseline, latest.clone())?;
            Ok(())
        })
    }

    fn lock_at(path: &Path) -> Result<std::fs::File> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.lock().context("lock config transaction")?;
        Ok(lock)
    }

    fn update_at(path: &Path, edit: impl FnOnce(&mut Self) -> Result<()>) -> Result<Self> {
        let _lock = Self::lock_at(path)?;
        // A partial edit must never replace unreadable preferences with defaults.
        // Only a genuinely absent file starts a new configuration.
        let mut config: Self = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).context("parse config before update")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error).context("read config before update"),
        };
        config.sanitize();
        edit(&mut config)?;
        config.sanitize();
        config.write_at(path)?;
        Ok(config)
    }

    fn save_at(&self, path: &Path) -> Result<()> {
        let _lock = Self::lock_at(path)?;
        self.write_at(path)
    }

    fn write_at(&self, path: &Path) -> Result<()> {
        slot_ports(self.video_port, self.input_port, self.max_tablets)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).context("serialize config")?;
        // Say exactly which lines change. Settings that drift with nobody
        // touching them are impossible to chase down otherwise.
        if let Ok(old) = std::fs::read_to_string(path) {
            let before: std::collections::BTreeMap<&str, &str> =
                old.lines().filter_map(|l| l.split_once(" = ")).collect();
            let changed: Vec<String> = text
                .lines()
                .filter_map(|l| l.split_once(" = "))
                .filter(|(k, v)| before.get(k) != Some(v))
                .map(|(k, v)| {
                    format!(
                        "{} = {} (was {})",
                        k,
                        v,
                        before.get(k).unwrap_or(&"<unset>")
                    )
                })
                .collect();
            if !changed.is_empty() {
                tracing::info!("Config written: {}", changed.join(", "));
            }
        }
        // Write-then-rename: a reader must never see a half-written file.
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().context("config directory")?)?;
        tmp.write_all(text.as_bytes())
            .context("write config file")?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).context("replace config file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t140_xdg_config_isolation() {
        if let Ok(mode) = std::env::var("USCREEN_T140_CHILD") {
            if mode == "absolute" {
                let expected = PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap())
                    .join("uscreen/config.toml");
                assert_eq!(
                    config_path(),
                    expected,
                    "config must stay inside test's XDG directory"
                );
                assert!(!FileConfig::load().check_updates);
                FileConfig::update(|config| {
                    config.fps = 30;
                    Ok(())
                })
                .unwrap();
                assert_eq!(FileConfig::load().fps, 30);
            } else {
                let expected = PathBuf::from(std::env::var_os("HOME").unwrap())
                    .join(".config/uscreen/config.toml");
                assert_eq!(config_path(), expected);
            }
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("uscreen")).unwrap();
        std::fs::write(
            temp.path().join("uscreen/config.toml"),
            "check_updates = false\n",
        )
        .unwrap();
        for (mode, value) in [
            ("absolute", temp.path()),
            ("relative", Path::new("relative-config")),
            ("empty", Path::new("")),
        ] {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::t140_xdg_config_isolation", "--nocapture"])
                .env("USCREEN_T140_CHILD", mode)
                .env("XDG_CONFIG_HOME", value)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{mode}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        assert_eq!(
            FileConfig::load_at(&temp.path().join("uscreen/config.toml")).fps,
            30
        );
    }

    #[test]
    fn t137_partial_updates_preserve_invalid_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "encoder = 'libx264'\nbitrate = broken\n";
        std::fs::write(&path, original).unwrap();
        let result = FileConfig::update_at(&path, |config| {
            config.fps = 30;
            Ok(())
        });
        assert!(result.is_err(), "invalid config must reject partial edits");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let mut edited = false;
        assert!(FileConfig::update_at(&path, |_| {
            edited = true;
            Ok(())
        })
        .is_err());
        assert!(!edited, "read failure must be detected before editing");
        assert!(path.is_dir());

        std::fs::remove_dir(&path).unwrap();
        let initialized = FileConfig::update_at(&path, |config| {
            config.fps = 30;
            Ok(())
        })
        .unwrap();
        assert_eq!(initialized.fps, 30);
        assert_eq!(FileConfig::load_at(&path).fps, 30);
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

    // T104: concurrent transactions retain every edit; readers never see partial TOML.
    #[test]
    fn t104_concurrent_config_transactions() {
        let dir = std::env::temp_dir().join(format!("uscreen-t104-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        FileConfig::default().save_at(&path).unwrap();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let barrier = &barrier;
            let path = &path;
            for _ in 0..8 {
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..4 {
                        FileConfig::update_at(path, |config| {
                            let old = config.bitrate;
                            std::thread::sleep(std::time::Duration::from_millis(5));
                            config.bitrate = old + 100;
                            Ok(())
                        })
                        .unwrap();
                        let text = std::fs::read_to_string(path).unwrap();
                        toml::from_str::<FileConfig>(&text).unwrap();
                    }
                });
            }
        });
        assert_eq!(FileConfig::load_at(&path).bitrate, 23_200);
        std::fs::remove_dir_all(dir).unwrap();
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
