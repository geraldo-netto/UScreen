use crate::capture::EncoderSettings;
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{accept_async_with_config, tungstenite::protocol::WebSocketConfig};
use tracing::{debug, error, info, warn};
use uscreen_config::commands::AsyncCommandExt;

// Linux input event constants
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;

const SYN_REPORT: u16 = 0x00;

const BTN_TOUCH: u16 = 0x14a;
const BTN_TOOL_FINGER: u16 = 0x145;
const BTN_TOOL_PEN: u16 = 0x140;
const BTN_TOOL_RUBBER: u16 = 0x141;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_PRESSURE: u16 = 0x18;
const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;
const ABS_MT_PRESSURE: u16 = 0x3a;
const ABS_TILT_X: u16 = 0x1a;
const ABS_TILT_Y: u16 = 0x1b;

const BTN_STYLUS: u16 = 0x14b;
const BTN_LEFT: u16 = 0x110;

// uinput ioctl constants (modern UI_DEV_SETUP/UI_ABS_SETUP API — the legacy
// uinput_user_dev write() API cannot declare axis resolution, which makes
// libinput reject the device: "missing tablet capabilities ... resolution")
const UI_SET_EVBIT: libc::c_ulong = 0x40045564;
const UI_SET_KEYBIT: libc::c_ulong = 0x40045565;
const UI_SET_ABSBIT: libc::c_ulong = 0x40045567;
const UI_SET_PROPBIT: libc::c_ulong = 0x4004556e;
const UI_DEV_SETUP: libc::c_ulong = 0x405c5503;
const UI_ABS_SETUP: libc::c_ulong = 0x401c5504;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;

const BUS_VIRTUAL: u16 = 0x06;
const INPUT_PROP_DIRECT: i32 = 0x01;

/// Coordinates are injected in a fixed 0..65535 space — the compositor maps
/// the device onto the output, so the virtual display resolution can change
/// at runtime without recreating uinput devices.
const COORD_MAX: i32 = 65535;

/// Identity of the virtual input devices. These must stay stable: KDE keys the
/// device→output association in kcminputrc on vendor/product/name, so changing
/// any of them silently orphans the mapping written by `map_devices_to_output`.
const UINPUT_VENDOR: u16 = 0x4553;
const PRODUCT_TOUCH: u16 = 0x0001;
const PRODUCT_PEN: u16 = 0x0002;
const PRODUCT_POINTER: u16 = 0x0003;
const TOUCH_DEVICE_NAME: &str = "UScreen Touch";
const PEN_DEVICE_NAME: &str = "UScreen Pen";
const POINTER_DEVICE_NAME: &str = "UScreen Pointer";

/// Device names and product ids for the N-th tablet. The first keeps the
/// original names and ids so existing KWin input mappings stay valid; every
/// further one gets a numbered name and its own product range, since KWin
/// keys its per-device settings on vendor/product/name.
#[derive(Clone, Debug)]
pub struct DeviceIdentity {
    pub touch: String,
    pub pen: String,
    pub pointer: String,
    pub product_touch: u16,
    pub product_pen: u16,
    pub product_pointer: u16,
}

impl DeviceIdentity {
    pub fn for_instance(instance: u32) -> Self {
        let suffix = if instance == 0 {
            String::new()
        } else {
            format!(" {}", instance + 1)
        };
        let base = (instance as u16) * 16;
        Self {
            touch: format!("{}{}", TOUCH_DEVICE_NAME, suffix),
            pen: format!("{}{}", PEN_DEVICE_NAME, suffix),
            pointer: format!("{}{}", POINTER_DEVICE_NAME, suffix),
            product_touch: PRODUCT_TOUCH + base,
            product_pen: PRODUCT_PEN + base,
            product_pointer: PRODUCT_POINTER + base,
        }
    }
    fn owns(&self, name: &str) -> bool {
        name == self.touch || name == self.pen || name == self.pointer
    }
}
/// ~310mm wide active area → 65535/310 ≈ 211 units/mm.
/// libinput requires a resolution on touchscreen/tablet axes.
const RESOLUTION_UNITS_PER_MM: i32 = 211;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InputAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    _pad: u16,
    absinfo: InputAbsInfo,
}

#[repr(C)]
struct UinputSetup {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[derive(Deserialize, Debug)]
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
    #[serde(rename = "mode")]
    Mode { pen_only: bool },
    /// Must be the first message on the socket. Proves the client is the
    /// tablet this daemon launched, not some other process on the loopback.
    #[serde(rename = "auth")]
    Auth { token: String },
    /// The tablet has this frame on screen. Closes the latency measurement
    /// loop — the host times encoded-packet readiness through acknowledgement
    /// receipt on its own clock, without clock agreement between devices.
    #[serde(rename = "rendered")]
    Rendered {
        seq: u32,
        /// Microseconds the tablet spent between receiving the frame and
        /// putting it on screen. Subtracting it leaves host queueing, delivery,
        /// and acknowledgement return time; it is not a pure transport measure.
        #[serde(default)]
        decode_us: i64,
    },
}

#[derive(Serialize)]
pub struct InputResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    pub width: u32,
    pub height: u32,
    /// Which bitstream the tablet should expect: "h264" or "hevc". It has to
    /// build the decoder before the first frame arrives, and the frames
    /// themselves carry nothing that identifies the codec.
    pub codec: String,
    /// Tells the tablet not to expect a video stream: it is acting as a
    /// graphics tablet for the host's own screen, not as a display.
    pub pen_only: bool,
    pub touch: bool,
    pub pen: bool,
}

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
    fn response(
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
                crate::capture::Codec::from_encoder(&current.encoder)
                    .muxer()
                    .to_string()
            })
            .unwrap_or_else(|| self.codec.clone());
        InputResponse {
            status: status.into(),
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

// Linux input_event struct (for writing to uinput)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct LinuxInputEvent {
    tv_sec: i64,
    tv_usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

/// A uinput virtual input device for injecting touch/pen events into Linux.
/// Touch and pen are SEPARATE devices: libinput classifies a touchscreen and
/// a tablet pen differently and rejects a device that mixes both.
struct UInputDevice {
    file: File,
}

impl UInputDevice {
    fn open_uinput() -> Result<File> {
        OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .context("Failed to open /dev/uinput. Ensure the uinput module is loaded and you have permissions (try: sudo modprobe uinput)")
    }

    /// Touchscreen device: multitouch + single-touch axes, INPUT_PROP_DIRECT.
    fn new_touch(name: &str, product: u16) -> Result<Self> {
        let file = Self::open_uinput()?;
        let fd = file.as_raw_fd();
        let w = COORD_MAX + 1;
        let h = COORD_MAX + 1;

        unsafe {
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_SYN as i32)?;
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_KEY as i32)?;
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_ABS as i32)?;
            Self::ioctl_val(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT)?;

            Self::ioctl_val(fd, UI_SET_KEYBIT, BTN_TOUCH as i32)?;
            Self::ioctl_val(fd, UI_SET_KEYBIT, BTN_TOOL_FINGER as i32)?;

            Self::abs_setup(fd, ABS_X, 0, w - 1, RESOLUTION_UNITS_PER_MM)?;
            Self::abs_setup(fd, ABS_Y, 0, h - 1, RESOLUTION_UNITS_PER_MM)?;
            Self::abs_setup(fd, ABS_PRESSURE, 0, 4096, 0)?;
            Self::abs_setup(fd, ABS_MT_SLOT, 0, 9, 0)?;
            Self::abs_setup(fd, ABS_MT_POSITION_X, 0, w - 1, RESOLUTION_UNITS_PER_MM)?;
            Self::abs_setup(fd, ABS_MT_POSITION_Y, 0, h - 1, RESOLUTION_UNITS_PER_MM)?;
            Self::abs_setup(fd, ABS_MT_TRACKING_ID, 0, 65535, 0)?;
            Self::abs_setup(fd, ABS_MT_PRESSURE, 0, 4096, 0)?;

            Self::dev_setup_and_create(fd, name, product)?;
        }

        info!("uinput touchscreen '{}' created", name);
        std::thread::sleep(std::time::Duration::from_millis(200));
        Ok(Self { file })
    }

    /// Pen tablet device: stylus tool + pressure + tilt.
    /// No INPUT_PROP_DIRECT — that flag means "touchscreen" and causes KDE to
    /// activate the on-screen keyboard on every pen tap. Without it, libinput
    /// classifies this as a tablet tool (Wacom-style): the cursor follows the
    /// pen position and clicks work as mouse clicks.
    fn new_pen(name: &str, product: u16) -> Result<Self> {
        let file = Self::open_uinput()?;
        let fd = file.as_raw_fd();
        let w = COORD_MAX + 1;
        let h = COORD_MAX + 1;

        unsafe {
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_SYN as i32)?;
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_KEY as i32)?;
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_ABS as i32)?;

            Self::ioctl_val(fd, UI_SET_KEYBIT, BTN_TOUCH as i32)?;
            Self::ioctl_val(fd, UI_SET_KEYBIT, BTN_TOOL_PEN as i32)?;
            // Eraser end: reported as TOOL_TYPE_ERASER on Android, mapped to
            // BTN_TOOL_RUBBER here so GIMP's eraser tool follows the pen.
            Self::ioctl_val(fd, UI_SET_KEYBIT, BTN_TOOL_RUBBER as i32)?;
            // libinput requires the stylus button capability on pen devices
            Self::ioctl_val(fd, UI_SET_KEYBIT, BTN_STYLUS as i32)?;

            Self::abs_setup(fd, ABS_X, 0, w - 1, RESOLUTION_UNITS_PER_MM)?;
            Self::abs_setup(fd, ABS_Y, 0, h - 1, RESOLUTION_UNITS_PER_MM)?;
            Self::abs_setup(fd, ABS_PRESSURE, 0, 4096, 0)?;
            // Tilt in whole degrees
            Self::abs_setup(fd, ABS_TILT_X, -90, 90, 0)?;
            Self::abs_setup(fd, ABS_TILT_Y, -90, 90, 0)?;

            Self::dev_setup_and_create(fd, name, product)?;
        }

        info!("uinput pen tablet '{}' created", name);
        std::thread::sleep(std::time::Duration::from_millis(200));
        Ok(Self { file })
    }

    /// An absolute-positioning pointer, the same shape as a VM's virtual
    /// tablet. It exists for one reason: a tablet tool's cursor is hidden the
    /// moment the tool leaves proximity, which is correct for a Wacom on a desk
    /// but wrong here — lift the pen and you lose all sense of where you were
    /// pointing. Parking this pointer at the last pen position leaves an
    /// ordinary mouse cursor sitting there.
    ///
    /// No INPUT_PROP_DIRECT (that would make it a touchscreen and bring the
    /// on-screen keyboard with it) and no BTN_TOOL_PEN (that would make it a
    /// second tablet).
    fn new_pointer(name: &str, product: u16) -> Result<Self> {
        let file = Self::open_uinput()?;
        let fd = file.as_raw_fd();
        let w = COORD_MAX + 1;

        unsafe {
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_SYN as i32)?;
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_KEY as i32)?;
            Self::ioctl_val(fd, UI_SET_EVBIT, EV_ABS as i32)?;
            Self::ioctl_val(fd, UI_SET_KEYBIT, BTN_LEFT as i32)?;
            Self::abs_setup(fd, ABS_X, 0, w - 1, RESOLUTION_UNITS_PER_MM)?;
            Self::abs_setup(fd, ABS_Y, 0, w - 1, RESOLUTION_UNITS_PER_MM)?;
            Self::dev_setup_and_create(fd, name, product)?;
        }

        info!("uinput pointer '{}' created", name);
        std::thread::sleep(std::time::Duration::from_millis(200));
        Ok(Self { file })
    }

    unsafe fn abs_setup(fd: i32, code: u16, min: i32, max: i32, resolution: i32) -> Result<()> {
        let setup = UinputAbsSetup {
            code,
            _pad: 0,
            absinfo: InputAbsInfo {
                value: 0,
                minimum: min,
                maximum: max,
                fuzz: 0,
                flat: 0,
                resolution,
            },
        };
        Self::ioctl_val(fd, UI_SET_ABSBIT, code as i32)?;
        if libc::ioctl(fd, UI_ABS_SETUP, &setup as *const UinputAbsSetup) < 0 {
            anyhow::bail!(
                "UI_ABS_SETUP({:#x}) failed: {}",
                code,
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }

    unsafe fn dev_setup_and_create(fd: i32, name: &str, product: u16) -> Result<()> {
        let mut setup = UinputSetup {
            bustype: BUS_VIRTUAL,
            vendor: UINPUT_VENDOR,
            product,
            version: 1,
            name: [0u8; 80],
            ff_effects_max: 0,
        };
        let name_bytes = name.as_bytes();
        let len = name_bytes.len().min(79);
        setup.name[..len].copy_from_slice(&name_bytes[..len]);

        if libc::ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup) < 0 {
            anyhow::bail!("UI_DEV_SETUP failed: {}", std::io::Error::last_os_error());
        }
        if libc::ioctl(fd, UI_DEV_CREATE) < 0 {
            anyhow::bail!("UI_DEV_CREATE failed: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }

    unsafe fn ioctl_val(fd: i32, request: libc::c_ulong, value: i32) -> Result<()> {
        if libc::ioctl(fd, request as libc::c_ulong, value) < 0 {
            anyhow::bail!(
                "ioctl({:#x}, {}) failed: {}",
                request,
                value,
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }

    fn emit(&mut self, type_: u16, code: u16, value: i32) -> Result<()> {
        let ev = LinuxInputEvent {
            tv_sec: 0,
            tv_usec: 0,
            type_,
            code,
            value,
        };
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                &ev as *const LinuxInputEvent as *const u8,
                std::mem::size_of::<LinuxInputEvent>(),
            )
        };
        self.file.write_all(bytes)?;
        Ok(())
    }

    fn syn(&mut self) -> Result<()> {
        self.emit(EV_SYN, SYN_REPORT, 0)?;
        self.file.flush()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn inject_pen(
        &mut self,
        x: i32,
        y: i32,
        pressure: i32,
        tilt_x: i32,
        tilt_y: i32,
        action: u8,
        eraser: bool,
    ) -> Result<()> {
        // The active tablet-tool key depends on which end of the pen is in
        // use. An S Pen flipped to its eraser end reports TOOL_TYPE_ERASER.
        let tool = if eraser {
            BTN_TOOL_RUBBER
        } else {
            BTN_TOOL_PEN
        };
        match action {
            0 => {
                // DOWN, in two frames.
                //
                // A tablet tool has to enter proximity before it can touch:
                // libinput wants to see the tool appear at a position, and only
                // then the tip come down. Announcing both in a single event
                // frame makes the first tap after picking up the pen land at
                // the previous cursor position, or get dropped entirely.
                self.emit(EV_KEY, tool, 1)?;
                self.emit(EV_ABS, ABS_X, x)?;
                self.emit(EV_ABS, ABS_Y, y)?;
                self.emit(EV_ABS, ABS_TILT_X, tilt_x)?;
                self.emit(EV_ABS, ABS_TILT_Y, tilt_y)?;
                self.syn()?;

                self.emit(EV_KEY, BTN_TOUCH, 1)?;
                self.emit(EV_ABS, ABS_PRESSURE, pressure)?;
                self.syn()?;
            }
            1 => {
                // UP carries the final sample. Publish it while the tool is
                // still in proximity: libinput ignores axes on proximity-out.
                self.emit(EV_ABS, ABS_X, x)?;
                self.emit(EV_ABS, ABS_Y, y)?;
                self.emit(EV_ABS, ABS_TILT_X, tilt_x)?;
                self.emit(EV_ABS, ABS_TILT_Y, tilt_y)?;
                self.emit(EV_KEY, BTN_TOUCH, 0)?;
                self.emit(EV_ABS, ABS_PRESSURE, 0)?;
                self.syn()?;

                self.emit(EV_KEY, tool, 0)?;
                self.syn()?;
            }
            2 => {
                // MOVE (pressing)
                self.emit(EV_ABS, ABS_X, x)?;
                self.emit(EV_ABS, ABS_Y, y)?;
                self.emit(EV_ABS, ABS_PRESSURE, pressure)?;
                self.emit(EV_ABS, ABS_TILT_X, tilt_x)?;
                self.emit(EV_ABS, ABS_TILT_Y, tilt_y)?;
                self.syn()?;
            }
            3 => {
                // HOVER — pen near screen, cursor follows without clicking.
                // Requires no INPUT_PROP_DIRECT on the device (we removed it)
                // so libinput classifies this as a tablet tool in proximity.
                self.emit(EV_KEY, tool, 1)?;
                self.emit(EV_ABS, ABS_X, x)?;
                self.emit(EV_ABS, ABS_Y, y)?;
                self.emit(EV_ABS, ABS_PRESSURE, 0)?;
                self.emit(EV_ABS, ABS_TILT_X, tilt_x)?;
                self.emit(EV_ABS, ABS_TILT_Y, tilt_y)?;
                self.syn()?;
            }
            4 => {
                // HOVER_EXIT — pen left proximity
                // Release the kernel key too: libinput clears its own button
                // state on proximity-out, but a latched uinput key would make
                // the kernel suppress the next press as a duplicate (T317).
                self.emit(EV_KEY, BTN_STYLUS, 0)?;
                self.emit(EV_KEY, BTN_TOUCH, 0)?;
                self.emit(EV_KEY, BTN_TOOL_PEN, 0)?;
                self.emit(EV_KEY, BTN_TOOL_RUBBER, 0)?;
                self.emit(EV_ABS, ABS_PRESSURE, 0)?;
                self.syn()?;
            }
            5 => {
                // STYLUS BUTTON DOWN (S Pen side button → right-click in GIMP)
                self.emit(EV_KEY, BTN_STYLUS, 1)?;
                self.syn()?;
            }
            6 => {
                // STYLUS BUTTON UP
                self.emit(EV_KEY, BTN_STYLUS, 0)?;
                self.syn()?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl Drop for UInputDevice {
    fn drop(&mut self) {
        unsafe {
            let fd = self.file.as_raw_fd();
            libc::ioctl(fd, UI_DEV_DESTROY as libc::c_ulong);
        }
        info!("uinput device destroyed");
    }
}

/// One virtual input device, or `None` with the reason logged: off in the
/// config (debug, it is a choice) or refused by uinput (warn, it is a fault).
fn create_device(
    enabled: bool,
    what: &str,
    make: impl FnOnce() -> Result<UInputDevice>,
) -> Option<UInputDevice> {
    if !enabled {
        debug!("{} device off by config", what);
        return None;
    }
    match make() {
        Ok(dev) => Some(dev),
        Err(e) => {
            warn!("No {} device: {}. {} input will be dropped.", what, e, what);
            None
        }
    }
}

/// How many attached tablets currently have a touch device. The on-screen
/// keyboard is suppressed while that is non-zero: the desktop sees a
/// touchscreen and would pop the keyboard over the screen used as a monitor.
static TOUCH_DEVICES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

async fn osk_touch_device_added() {
    TOUCH_DEVICES.fetch_add(1, Ordering::SeqCst);
    crate::osk::sync_touch_state(&TOUCH_DEVICES).await;
}

async fn osk_touch_device_removed() {
    TOUCH_DEVICES.fetch_sub(1, Ordering::SeqCst);
    crate::osk::sync_touch_state(&TOUCH_DEVICES).await;
}

/// Own device lifetime even when the watcher is cancelled inside an await.
struct DeviceOwner {
    devices: Arc<std::sync::Mutex<InjectDevices>>,
    touch_registered: bool,
}
impl DeviceOwner {
    async fn create_devices(&mut self, cfg: &InputConfig, ident: &DeviceIdentity) -> usize {
        let (c, i) = (cfg.clone(), ident.clone());
        // Device creation sleeps to let udev settle, so it
        // runs off the async runtime.
        let created = tokio::task::spawn_blocking(move || InjectDevices::create(&c, &i))
            .await
            .unwrap_or_else(|_| InjectDevices::empty());
        let count = created.count();
        let has_touch = created.touch.is_some();
        if let Ok(mut guard) = self.devices.lock() {
            *guard = created;
        }
        if has_touch {
            self.touch_registered = true;
            osk_touch_device_added().await;
        }
        count
    }

    async fn remove_devices(&mut self) {
        let old = self
            .devices
            .lock()
            .map(|mut g| {
                g.release_all();
                std::mem::replace(&mut *g, InjectDevices::empty())
            })
            .ok();
        let had_touch = old.as_ref().is_some_and(|d| d.touch.is_some());
        drop(old);
        if had_touch {
            self.touch_registered = false;
            osk_touch_device_removed().await;
        }
        info!("Tablet detached — virtual input devices removed");
    }

    fn release_for_remap(&self, pen_only: bool, card: Option<u32>) -> usize {
        let count = self
            .devices
            .lock()
            .map(|mut g| {
                g.release_all();
                g.count()
            })
            .unwrap_or(0);
        if count > 0 {
            info!(
                "Mode is now {} — remapping input devices{}",
                if pen_only {
                    "pen-only"
                } else {
                    "second screen"
                },
                card.map(|c| format!(" (card{})", c)).unwrap_or_default()
            );
        }
        count
    }
}

impl Drop for DeviceOwner {
    fn drop(&mut self) {
        if let Ok(mut devices) = self.devices.lock() {
            devices.release_all();
            *devices = InjectDevices::empty();
        }
        if self.touch_registered {
            TOUCH_DEVICES.fetch_sub(1, Ordering::SeqCst);
            // Drop cannot await; recovery state remains on disk if runtime shutdown
            // prevents this final restoration from completing.
            tokio::spawn(crate::osk::sync_touch_state(&TOUCH_DEVICES));
        }
    }
}

/// The virtual input devices backing one tablet connection.
struct InjectDevices {
    touch: Option<UInputDevice>,
    pen: Option<UInputDevice>,
    /// Takes over the cursor when the pen leaves proximity, so it stays where
    /// the user last pointed instead of vanishing.
    pointer: Option<UInputDevice>,
    last_pen_pos: (i32, i32),
    /// Bitmask of MT slots that currently have an active tracking ID
    /// (DOWN received, no matching UP yet). Bit N identifies slot N, 0–9.
    active_slots: u16,
    touch_contacts: [Option<(i32, i32, i32)>; 10],
    pen_proximity: bool,
    /// S Pen side button currently held (BTN_STYLUS). Tracked so a held
    /// button is released cleanly if the connection drops.
    pen_button: bool,
}

impl InjectDevices {
    fn empty() -> Self {
        Self {
            touch: None,
            pen: None,
            pointer: None,
            last_pen_pos: (0, 0),
            active_slots: 0,
            touch_contacts: [None; 10],
            pen_proximity: false,
            pen_button: false,
        }
    }

    /// Creates whichever devices the config asks for. The pointer only
    /// exists to park the cursor when the pen lifts, so it is tied to the
    /// pen here and nowhere else has to know that rule.
    fn create(cfg: &InputConfig, ident: &DeviceIdentity) -> Self {
        let touch = create_device(cfg.touch, "touch", || {
            UInputDevice::new_touch(&ident.touch, ident.product_touch)
        });
        let pen = create_device(cfg.pen, "pen", || {
            UInputDevice::new_pen(&ident.pen, ident.product_pen)
        });
        let pointer = create_device(cfg.pen && cfg.pointer, "pointer", || {
            UInputDevice::new_pointer(&ident.pointer, ident.product_pointer)
        });
        if pen.is_some() && pointer.is_none() {
            info!("No pointer device — the cursor will vanish when the pen lifts");
        }
        Self {
            touch,
            pen,
            pointer,
            ..Self::empty()
        }
    }

    fn count(&self) -> usize {
        self.touch.is_some() as usize
            + self.pen.is_some() as usize
            + self.pointer.is_some() as usize
    }

    fn inject_touch(&mut self, x: i32, y: i32, pressure: i32, action: u8, slot: u8) -> Result<()> {
        let Some(contact) = self.touch_contacts.get_mut(slot as usize) else {
            return Ok(());
        };
        let Some(dev) = self.touch.as_mut() else {
            return Ok(());
        };
        match action {
            0 => {
                *contact = Some((x, y, pressure));
                self.active_slots |= 1 << slot;
            }
            1 if contact.is_some() => {
                *contact = None;
                self.active_slots &= !(1 << slot);
            }
            2 if contact.is_some() => {
                *contact = Some((x, y, pressure));
            }
            _ => return Ok(()),
        }
        dev.emit(EV_ABS, ABS_MT_SLOT, slot as i32)?;
        match action {
            0 => dev.emit(EV_ABS, ABS_MT_TRACKING_ID, slot as i32)?,
            1 => dev.emit(EV_ABS, ABS_MT_TRACKING_ID, -1)?,
            _ => {}
        }
        if action != 1 {
            dev.emit(EV_ABS, ABS_MT_POSITION_X, x)?;
            dev.emit(EV_ABS, ABS_MT_POSITION_Y, y)?;
            dev.emit(EV_ABS, ABS_MT_PRESSURE, pressure)?;
        }
        // Legacy single-touch consumers follow the first remaining contact.
        let primary = self.touch_contacts.iter().flatten().next();
        dev.emit(EV_KEY, BTN_TOUCH, i32::from(primary.is_some()))?;
        dev.emit(EV_KEY, BTN_TOOL_FINGER, i32::from(primary.is_some()))?;
        if let Some(&(x, y, pressure)) = primary {
            dev.emit(EV_ABS, ABS_X, x)?;
            dev.emit(EV_ABS, ABS_Y, y)?;
            dev.emit(EV_ABS, ABS_PRESSURE, pressure)?;
        } else {
            dev.emit(EV_ABS, ABS_PRESSURE, 0)?;
        }
        dev.syn()
    }

    /// Release all active contacts cleanly before the connection closes.
    /// Without this, a stuck MT slot or a pen left in proximity causes
    /// the next connection to inherit phantom input events.
    fn release_all(&mut self) {
        if let Some(ref mut dev) = self.touch {
            for slot in 0u8..16 {
                if self.active_slots & (1u16 << slot) != 0 {
                    let _ = dev.emit(EV_ABS, ABS_MT_SLOT, slot as i32);
                    let _ = dev.emit(EV_ABS, ABS_MT_TRACKING_ID, -1);
                }
            }
            if self.active_slots != 0 {
                let _ = dev.emit(EV_KEY, BTN_TOUCH, 0);
                let _ = dev.emit(EV_KEY, BTN_TOOL_FINGER, 0);
                let _ = dev.emit(EV_ABS, ABS_PRESSURE, 0);
                let _ = dev.syn();
            }
        }
        self.active_slots = 0;
        self.touch_contacts = [None; 10];

        if self.pen_proximity {
            if let Some(ref mut dev) = self.pen {
                let _ = dev.emit(EV_KEY, BTN_TOUCH, 0);
                let _ = dev.emit(EV_KEY, BTN_TOOL_PEN, 0);
                let _ = dev.emit(EV_KEY, BTN_TOOL_RUBBER, 0);
                let _ = dev.emit(EV_ABS, ABS_PRESSURE, 0);
                let _ = dev.syn();
            }
        }
        self.pen_proximity = false;

        if self.pen_button {
            if let Some(ref mut dev) = self.pen {
                let _ = dev.emit(EV_KEY, BTN_STYLUS, 0);
                let _ = dev.syn();
            }
        }
        self.pen_button = false;
    }
}

const KWIN_INPUT_IFACE: &str = "org.kde.KWin.InputDevice";

/// The screen the user is actually looking at: the first enabled output that is
/// not one of ours. In pen-only mode the tablet drives this one, so the pen has
/// to be mapped onto it rather than onto the virtual display.
async fn primary_non_evdi_output() -> Option<String> {
    let evdi: Vec<String> = crate::vdisplay::evdi_connectors()
        .into_iter()
        .map(|c| c.name)
        .collect();
    let out = tokio::process::Command::new("kscreen-doctor")
        .arg("-j")
        .output_bounded()
        .await
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let outputs = v.get("outputs")?.as_array()?;
    let mut fallback = None;
    for o in outputs {
        let name = o.get("name").and_then(|v| v.as_str())?.to_string();
        if evdi.contains(&name) || !o.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        if o.get("primary").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Some(name);
        }
        fallback.get_or_insert(name);
    }
    fallback
}

async fn kwin_device_property(sysname: &str, property: &str) -> Option<String> {
    crate::kwin::get_property(
        &format!("/org/kde/KWin/InputDevice/{}", sysname),
        KWIN_INPUT_IFACE,
        property,
    )
    .await
}

/// Pin the virtual input devices to the virtual display.
///
/// An absolute-positioning device is meaningless without knowing which screen
/// it addresses. Left unmapped, libinput spreads it across the whole desktop:
/// touching the middle of the tablet lands the cursor somewhere on the laptop
/// panel, and drawing with the pen goes to the wrong monitor entirely.
///
/// This is done over KWin's D-Bus interface rather than by writing kcminputrc.
/// Writing the config file looks like the obvious route and does produce the
/// documented `[Libinput][vendor][product][name] OutputName=` entry, but KWin
/// does not apply it to these devices — verified by reading the property back
/// and finding it empty, both when written before and after device creation.
/// Setting the property directly takes effect immediately, and KWin persists it
/// itself.
/// `expected` is how many devices this instance actually created; the retry
/// loop stops once that many are mapped rather than assuming all three exist.
async fn map_devices_to_output(
    pen_only: bool,
    ident: &DeviceIdentity,
    card: Option<u32>,
    expected: usize,
) {
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    map_devices_using(
        pen_only,
        ident,
        card,
        expected,
        &session_type,
        "xinput",
        "xrandr",
        None,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn map_devices_using(
    pen_only: bool,
    ident: &DeviceIdentity,
    card: Option<u32>,
    expected: usize,
    session_type: &str,
    xinput: &str,
    xrandr: &str,
    connectors: Option<&[crate::vdisplay::EvdiConnector]>,
) {
    if expected == 0 {
        return;
    }
    if session_type == "x11" {
        map_x11_devices(pen_only, ident, card, expected, xinput, xrandr, connectors).await;
        return;
    }
    let Some(output) = target_output(pen_only, card, std::time::Duration::from_secs(10)).await
    else {
        if pen_only {
            warn!("No physical output found — pen will address the whole desktop");
        } else {
            warn!("No EVDI output found — touch and pen will address the whole desktop");
        }
        return;
    };

    // KWin registers a device slightly after uinput creates it, so retry
    // rather than racing it. The same loop also covers a mapping that KWin
    // accepted but did not keep, which is what the read-back below catches.
    for attempt in 0..20 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        let Some(devices) = crate::kwin::list_strings(
            "/org/kde/KWin/InputDevice",
            "org.kde.KWin.InputDeviceManager",
            "devicesSysNames",
        )
        .await
        else {
            warn!("KWin did not answer — input devices stay unmapped");
            return;
        };

        let mut mapped = 0;
        for sysname in devices {
            mapped += usize::from(map_kwin_device(&sysname, ident, &output).await);
        }

        if mapped >= expected {
            return;
        }
    }

    warn!("Input devices did not appear in KWin within 5s — mapping skipped");
}

async fn map_kwin_device(sysname: &str, ident: &DeviceIdentity, output: &str) -> bool {
    let Some(name) = kwin_device_property(sysname, "name").await else {
        return false;
    };
    if !ident.owns(&name) {
        return false;
    }
    let ok = crate::kwin::set_property(
        &format!("/org/kde/KWin/InputDevice/{}", sysname),
        KWIN_INPUT_IFACE,
        "outputName",
        "s",
        output,
    )
    .await;
    match ok {
        true => {
            // A successful Set is not proof: KWin answers ok and then
            // keeps the old value when the output is not usable yet.
            // Only what reads back counts, so a lost mapping is
            // retried on the next pass instead of logged as done.
            match kwin_device_property(sysname, "outputName").await {
                Some(now) if now == output => {
                    info!("Mapped '{}' ({}) to output {}", name, sysname, output);
                    return true;
                }
                now => warn!(
                    "Mapping '{}' to {} did not take (KWin reports {:?}) — retrying",
                    name,
                    output,
                    now.unwrap_or_default()
                ),
            }
        }
        false => warn!(
            "Could not map '{}': KWin refused the outputName property",
            name
        ),
    }
    false
}

fn x11_connector_matches(output: &str, connector: &str) -> bool {
    output == connector
        || output
            .strip_prefix(connector)
            .and_then(|s| s.strip_prefix('-'))
            .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
}

/// Xorg can expose a pen as separate pen/eraser devices. Keep tablet suffixes
/// exact: "UScreen Pen 2" must never match the first tablet's "UScreen Pen".
fn x11_device_kind<'a>(name: &str, ident: &'a DeviceIdentity) -> Option<&'a str> {
    if name == ident.touch {
        return Some(&ident.touch);
    }
    if name == ident.pointer {
        return Some(&ident.pointer);
    }
    if name == ident.pen
        || name
            .strip_prefix(&ident.pen)
            .is_some_and(|suffix| suffix.starts_with(" Pen (") || suffix.starts_with(" Eraser ("))
    {
        return Some(&ident.pen);
    }
    None
}

#[allow(clippy::too_many_arguments)]
async fn map_x11_devices(
    pen_only: bool,
    ident: &DeviceIdentity,
    card: Option<u32>,
    expected: usize,
    xinput: &str,
    xrandr: &str,
    fixed_connectors: Option<&[crate::vdisplay::EvdiConnector]>,
) {
    for attempt in 0..40 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        // Refresh on every retry: the helper may still be enabling its card.
        let current = crate::vdisplay::evdi_connectors();
        let connectors = fixed_connectors.unwrap_or(&current);
        let Some(randr) = x11_query(
            xrandr,
            &["--query"],
            "xrandr",
            "xrandr could not query this X11 session",
        )
        .await
        else {
            return;
        };
        let text = String::from_utf8_lossy(&randr.stdout);
        let active = x11_active_outputs(&text);
        let Some(output) = x11_target_output(pen_only, &active, connectors, card) else {
            continue;
        };
        let Some(devices) = x11_query(
            xinput,
            &["list", "--short"],
            "xinput",
            "xinput could not list input devices",
        )
        .await
        else {
            return;
        };
        if map_x11_list(
            &String::from_utf8_lossy(&devices.stdout),
            ident,
            xinput,
            output,
            expected,
        )
        .await
        {
            return;
        }
    }
    warn!("X11 output or input devices not ready after 10s; check xrandr providers and xinput");
}

async fn x11_query(
    program: &str,
    args: &[&str],
    tool: &str,
    failure: &str,
) -> Option<std::process::Output> {
    let Ok(output) = tokio::process::Command::new(program)
        .args(args)
        .output_bounded()
        .await
    else {
        warn!("X11 input mapping needs {}", tool);
        return None;
    };
    if !output.status.success() {
        warn!("{}", failure);
        return None;
    }
    Some(output)
}

fn x11_has_geometry(field: &str) -> bool {
    field.split_once('x').is_some_and(|(w, h)| {
        w.parse::<u32>().is_ok()
            && h.split(['+', '-'])
                .next()
                .is_some_and(|h| h.parse::<u32>().is_ok())
            && (h.contains('+') || h.contains('-'))
    })
}

fn x11_active_outputs(text: &str) -> Vec<(&str, bool)> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.get(1) != Some(&"connected") {
                return None;
            }
            fields
                .iter()
                .any(|field| x11_has_geometry(field))
                .then_some((fields[0], fields.contains(&"primary")))
        })
        .collect()
}

fn x11_capture_output(
    name: &str,
    connectors: &[crate::vdisplay::EvdiConnector],
    card: Option<u32>,
) -> bool {
    connectors.iter().any(|c| {
        c.connected
            && card.is_none_or(|want| c.card == want)
            && x11_connector_matches(name, &c.name)
    })
}

fn x11_target_output<'a>(
    pen_only: bool,
    active: &[(&'a str, bool)],
    connectors: &[crate::vdisplay::EvdiConnector],
    card: Option<u32>,
) -> Option<&'a str> {
    if pen_only {
        active
            .iter()
            .filter(|(name, _)| {
                !connectors
                    .iter()
                    .any(|c| x11_connector_matches(name, &c.name))
            })
            .max_by_key(|(_, primary)| primary)
            .map(|(name, _)| *name)
    } else {
        let candidates: Vec<_> = active
            .iter()
            .filter(|(name, _)| x11_capture_output(name, connectors, card))
            .map(|(name, _)| *name)
            .collect();
        // Ambiguous names must not attach input to another tablet.
        if candidates.len() == 1 {
            candidates.first().copied()
        } else {
            None
        }
    }
}

fn x11_list_entry<'a, 'b>(
    line: &'a str,
    ident: &'b DeviceIdentity,
) -> Option<(&'a str, &'a str, &'b str)> {
    let start = line.find("UScreen ")?;
    let (name, rest) = line[start..].split_once("id=")?;
    let kind = x11_device_kind(name.trim(), ident)?;
    let id = rest
        .split_whitespace()
        .next()
        .filter(|s| s.parse::<u32>().is_ok())?;
    Some((id, name.trim(), kind))
}

async fn map_x11_list(
    text: &str,
    ident: &DeviceIdentity,
    xinput: &str,
    output: &str,
    expected: usize,
) -> bool {
    let mut mapped = std::collections::HashSet::new();
    let mut failed = false;
    for line in text.lines() {
        let Some((id, name, kind)) = x11_list_entry(line, ident) else {
            continue;
        };
        let ok = tokio::process::Command::new(xinput)
            .args(["map-to-output", id, output])
            .output_bounded()
            .await
            .is_ok_and(|out| out.status.success());
        if ok {
            mapped.insert(kind);
            info!("Mapped '{}' (X11 id {}) to {}", name, id, output);
        } else {
            failed = true;
        }
    }
    !failed && mapped.len() >= expected
}

/// The output the devices should address in this mode, returned only once
/// KWin lists it as enabled.
///
/// Mapping is not something that can be done ahead of time: set `outputName`
/// while the virtual display is still being brought back and KWin keeps the
/// previous mapping, so after leaving graphics-tablet mode the pen and touch
/// would go on driving the laptop screen (issue #6). Leaving display mode has
/// the opposite problem — the physical screen is always there, but the EVDI
/// output is going away at that moment. So this polls the output list up to
/// `timeout` and only then falls back to the best name it knows, so a mapping
/// is at least attempted on a desktop where kscreen-doctor cannot answer.
async fn target_output(
    pen_only: bool,
    card: Option<u32>,
    timeout: std::time::Duration,
) -> Option<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if pen_only {
            if let Some(name) = primary_non_evdi_output().await {
                return Some(name);
            }
            // No kscreen-doctor means no KDE session: nothing to wait for.
            crate::kscreen::outputs().await?;
        } else {
            let connectors = crate::vdisplay::evdi_connectors();
            // Keep this tablet's assigned card, including during discovery gaps.
            let fallback = fallback_output(&connectors, card);
            let Some(outputs) = crate::kscreen::outputs().await else {
                return fallback;
            };
            let enabled = enabled_named_output(&outputs, fallback.as_deref());
            if let Some(o) = enabled {
                return o.get("name").and_then(|v| v.as_str()).map(str::to_string);
            }
            if tokio::time::Instant::now() >= deadline {
                if fallback.is_some() {
                    warn!(
                        "EVDI output not enabled within {:?} — mapping onto {:?} anyway",
                        timeout, fallback
                    );
                }
                return fallback;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

fn enabled_named_output<'a>(
    outputs: &'a [serde_json::Value],
    name: Option<&str>,
) -> Option<&'a serde_json::Value> {
    outputs.iter().find(|output| {
        output.get("name").and_then(|value| value.as_str()) == name
            && output
                .get("enabled")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
    })
}

fn fallback_output(
    connectors: &[crate::vdisplay::EvdiConnector],
    card: Option<u32>,
) -> Option<String> {
    // A known card must remain ours even while its connector is absent. With
    // no card yet, wait unless there is exactly one possible connector.
    match card {
        Some(card) => connectors.iter().find(|connector| connector.card == card),
        None if connectors.len() == 1 => connectors.first(),
        None => None,
    }
    .map(|connector| connector.name.clone())
}

struct Controllers {
    devices: Arc<std::sync::Mutex<InjectDevices>>,
    generation: watch::Sender<u64>,
}
impl Controllers {
    fn new(devices: Arc<std::sync::Mutex<InjectDevices>>) -> Self {
        Self {
            devices,
            generation: watch::channel(0).0,
        }
    }
    fn claim(self: &Arc<Self>) -> ControllerLease {
        let mut id = 0;
        self.generation.send_modify(|generation| {
            if let Ok(mut devices) = self.devices.lock() {
                devices.release_all();
            }
            *generation = generation.wrapping_add(1);
            id = *generation;
        });
        ControllerLease {
            controllers: self.clone(),
            id,
        }
    }
}
struct ControllerLease {
    controllers: Arc<Controllers>,
    id: u64,
}
impl Drop for ControllerLease {
    fn drop(&mut self) {
        // Keep the generation read lock through release, so a new claim cannot
        // interleave after the ownership check and before device cleanup.
        let generation = self.controllers.generation.borrow();
        if *generation == self.id {
            if let Ok(mut devices) = self.controllers.devices.lock() {
                devices.release_all();
            }
        }
    }
}

pub struct InputServer {
    config: InputConfig,
    running: Arc<AtomicBool>,
    settings_tx: Option<watch::Sender<EncoderSettings>>,
    /// Which mode the daemon is in, and how the tablet changes it. Owned as a
    /// channel rather than a field because the mode is switchable at runtime
    /// and several parts of the daemon have to follow it.
    mode_tx: watch::Sender<bool>,
    latency: crate::latency::LatencyTracker,
    /// Woken when a client fails to authenticate, so the daemon can push the
    /// token to the app again over adb (a manually launched app never got
    /// one).
    relaunch: Arc<tokio::sync::Notify>,
    /// The EVDI card this tablet's helper opened, once known. The devices are
    /// mapped onto that card's connector, not onto "the first EVDI output" -
    /// with two tablets that would put both pens on one screen.
    card_rx: watch::Receiver<Option<u32>>,
    /// Whether this tablet is attached at all. The virtual input devices
    /// exist exactly while it is.
    tablet_rx: watch::Receiver<bool>,
}

impl InputServer {
    pub fn new(
        config: InputConfig,
        settings_tx: Option<watch::Sender<EncoderSettings>>,
        mode_tx: watch::Sender<bool>,
        latency: crate::latency::LatencyTracker,
        relaunch: Arc<tokio::sync::Notify>,
        card_rx: watch::Receiver<Option<u32>>,
        tablet_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            settings_tx,
            mode_tx,
            latency,
            relaunch,
            card_rx,
            tablet_rx,
        }
    }

    pub async fn bind(&self) -> Result<TcpListener> {
        let addr = format!("127.0.0.1:{}", self.config.port);
        let listener = TcpListener::bind(&addr)
            .await
            .context(format!("Failed to bind input server to {}", addr))?;
        info!("Input server on ws://{}", addr);
        Ok(listener)
    }

    pub async fn run_with_listener(&self, listener: TcpListener) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        // The devices exist only while a tablet is attached. Created for the
        // daemon's whole lifetime they left a touchscreen and a pen tablet on
        // the desktop with nothing behind them, and merely having those
        // present changes desktop behaviour (Cinnamon and GNOME on X11 hide
        // the mouse cursor around touch devices). Recreating them on attach is
        // safe: DeviceIdentity is fixed per instance, names and product ids
        // alike, so the desktop's per-device settings and the output mapping
        // below find the same device every time.
        let ident = DeviceIdentity::for_instance(self.config.instance);
        if !self.config.any_device() {
            info!(
                "Virtual input devices are all off (input_touch / input_pen / \
                 input_pointer in config.toml) — the tablet is display-only"
            );
        }
        let uinput = Arc::new(std::sync::Mutex::new(InjectDevices::empty()));
        let controllers = Arc::new(Controllers::new(uinput.clone()));
        let mut tasks = tokio::task::JoinSet::new();
        let slots = Arc::new(tokio::sync::Semaphore::new(16));

        // Follow the tablet, the mode and the card for as long as the daemon
        // runs. Attach creates the devices and maps them; detach destroys
        // them; a mode or card switch moves them onto the other output and
        // drops anything held at that moment — a finger or pen tip that was
        // down would otherwise stay down on a screen no longer listening.
        tasks.spawn(follow_input_devices(
            self.tablet_rx.clone(),
            self.mode_tx.subscribe(),
            self.card_rx.clone(),
            uinput.clone(),
            ident,
            self.config.clone(),
        ));

        let config = self.config.clone();
        let running = self.running.clone();

        loop {
            let accept = tokio::select! {
                res = listener.accept() => res,
                _ = tasks.join_next(), if !tasks.is_empty() => continue,
                _ = async {
                    while running.load(Ordering::SeqCst) {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                } => break,
            };

            let (socket, peer) = match accept {
                Ok(s) => s,
                Err(e) => {
                    error!("Input accept failed: {}", e);
                    continue;
                }
            };

            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let incoming = PendingInput::new(socket);
            info!("Input client: {}", peer);
            let cfg = config.clone();
            let settings = self.settings_tx.clone();
            let mode_tx = self.mode_tx.clone();
            let latency = self.latency.clone();
            let devices = controllers.clone();
            let relaunch = self.relaunch.clone();
            tasks.spawn(async move {
                let _permit = permit;
                if let Err(e) =
                    handle_connection(incoming, cfg, settings, mode_tx, latency, devices, relaunch)
                        .await
                {
                    warn!("Input handler {}: {}", peer, e);
                }
            });
        }

        tasks.shutdown().await;
        Ok(())
    }
}

async fn follow_input_devices(
    mut tablet_rx: watch::Receiver<bool>,
    mut mode_rx: watch::Receiver<bool>,
    mut card_rx: watch::Receiver<Option<u32>>,
    devices: Arc<std::sync::Mutex<InjectDevices>>,
    ident: DeviceIdentity,
    cfg: InputConfig,
) {
    let mut owner = DeviceOwner {
        devices: devices.clone(),
        touch_registered: false,
    };
    let mut present = false;
    tablet_rx.borrow_and_update();
    mode_rx.borrow_and_update();
    card_rx.borrow_and_update();
    loop {
        let attached = *tablet_rx.borrow();
        let pen_only = *mode_rx.borrow();
        let card = *card_rx.borrow();
        let count;
        if attached && !present {
            present = true;
            count = owner.create_devices(&cfg, &ident).await;
        } else if !attached && present {
            present = false;
            owner.remove_devices().await;
            count = 0;
        } else if attached {
            count = owner.release_for_remap(pen_only, card);
        } else {
            count = 0;
        }
        // Leaving display mode tears the virtual output down and
        // entering it brings the output back. map_devices_to_output
        // waits for the output it needs to actually be enabled, so
        // a change during the wait simply restarts it.
        if !wait_for_mapping_change(&mut tablet_rx, &mut mode_rx, &mut card_rx, async {
            if count > 0 {
                map_devices_to_output(pen_only, &ident, card, count).await;
            }
        })
        .await
        {
            break;
        }
    }
}

async fn wait_for_mapping_change(
    tablet: &mut watch::Receiver<bool>,
    mode: &mut watch::Receiver<bool>,
    card: &mut watch::Receiver<Option<u32>>,
    mapping: impl std::future::Future<Output = ()>,
) -> bool {
    let changed = async {
        tokio::select! {
            biased;
            result = tablet.changed() => result.is_ok(),
            result = mode.changed() => result.is_ok(),
            result = card.changed() => result.is_ok(),
        }
    };
    tokio::pin!(changed);
    tokio::select! {
        biased;
        result = &mut changed => result,
        _ = mapping => changed.await,
    }
}

struct PendingInput {
    stream: tokio::net::TcpStream,
    deadline: tokio::time::Instant,
}
impl PendingInput {
    fn new(stream: tokio::net::TcpStream) -> Self {
        Self {
            stream,
            deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(3),
        }
    }
}

async fn handle_connection(
    incoming: PendingInput,
    config: InputConfig,
    settings_tx: Option<watch::Sender<EncoderSettings>>,
    mode_tx: watch::Sender<bool>,
    latency: crate::latency::LatencyTracker,
    controllers: Arc<Controllers>,
    relaunch: Arc<tokio::sync::Notify>,
) -> Result<()> {
    // Input events are a few hundred bytes. The library default of 64 MiB
    // per message is a memory bill nobody on this socket should be able to
    // run up.
    let ws_cfg = WebSocketConfig {
        max_message_size: Some(64 * 1024),
        max_frame_size: Some(64 * 1024),
        ..Default::default()
    };
    let deadline = incoming.deadline;
    let ws_stream = tokio::time::timeout_at(
        deadline,
        accept_async_with_config(incoming.stream, Some(ws_cfg)),
    )
    .await
    .context("WebSocket handshake timed out")?
    .context("WebSocket handshake failed")?;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    if !authenticate_input(
        &mut ws_sender,
        &mut ws_receiver,
        config.token.as_deref(),
        deadline,
        &relaunch,
    )
    .await
    {
        return Ok(());
    }

    serve_controller(
        ws_sender,
        ws_receiver,
        config,
        settings_tx,
        mode_tx,
        latency,
        controllers,
    )
    .await
}

async fn serve_controller(
    mut ws_sender: futures_util::stream::SplitSink<InputSocket, Message>,
    mut ws_receiver: futures_util::stream::SplitStream<InputSocket>,
    config: InputConfig,
    settings_tx: Option<watch::Sender<EncoderSettings>>,
    mode_tx: watch::Sender<bool>,
    latency: crate::latency::LatencyTracker,
    controllers: Arc<Controllers>,
) -> Result<()> {
    let mut mode_rx = mode_tx.subscribe();
    let mut settings_rx = settings_tx.as_ref().map(watch::Sender::subscribe);
    let mut ownership = controllers.generation.subscribe();
    let lease = controllers.claim();
    if *ownership.borrow_and_update() != lease.id {
        return Ok(());
    }

    let resp = config.response("connected", *mode_rx.borrow_and_update(), &settings_tx);

    if !send_controller_message(
        &mut ws_sender,
        Message::Text(serde_json::to_string(&resp)?),
        &mut ownership,
    )
    .await?
    {
        return Ok(());
    }

    loop {
        let msg = tokio::select! {
            biased;
            _ = ownership.changed() => break,
            incoming = ws_receiver.next() => match incoming {
                Some(m) => m,
                None => break,
            },
            changed = async {
                match settings_rx.as_mut() {
                    Some(rx) => rx.changed().await,
                    None => std::future::pending().await,
                }
            } => {
                if changed.is_err() { settings_rx = None; continue; }
                let response = config.response("mode", *mode_rx.borrow(), &settings_tx);
                if !send_controller_message(&mut ws_sender,
                    Message::Text(serde_json::to_string(&response)?), &mut ownership)
                    .await.unwrap_or(false) {
                    break;
                }
                continue;
            }
            // The mode changed — here, from the GUI, or from the command line.
            // Whoever changed it, the tablet has to hear about it: it decides
            // from this whether to expect a video stream at all.
            changed = mode_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let pen_only = *mode_rx.borrow();
                let resp = config.response("mode", pen_only, &settings_tx);
                if !send_controller_message(&mut ws_sender,
                    Message::Text(serde_json::to_string(&resp)?), &mut ownership)
                    .await.unwrap_or(false)
                {
                    break;
                }
                continue;
            }
        };

        match msg {
            Ok(Message::Text(text)) => {
                if !dispatch_controller_text(
                    &text,
                    &controllers,
                    lease.id,
                    &settings_tx,
                    &mode_tx,
                    &latency,
                    config.pen,
                ) {
                    break;
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(Message::Ping(data)) => {
                if !send_controller_message(&mut ws_sender, Message::Pong(data), &mut ownership)
                    .await
                    .unwrap_or(false)
                {
                    break;
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn dispatch_controller_text(
    text: &str,
    controllers: &Controllers,
    lease: u64,
    settings_tx: &Option<watch::Sender<EncoderSettings>>,
    mode_tx: &watch::Sender<bool>,
    latency: &crate::latency::LatencyTracker,
    pen_enabled: bool,
) -> bool {
    match serde_json::from_str::<InputEvent>(text) {
        Ok(event) => {
            let generation = controllers.generation.borrow();
            if *generation != lease {
                return false;
            }
            handle_event(
                event,
                &controllers.devices,
                settings_tx,
                mode_tx,
                latency,
                pen_enabled,
            );
        }
        Err(e) => warn!("Invalid input: {} - {}", e, text),
    }
    true
}

type InputSocket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn send_controller_message(
    sender: &mut futures_util::stream::SplitSink<InputSocket, Message>,
    message: Message,
    ownership: &mut watch::Receiver<u64>,
) -> Result<bool> {
    // A non-reading retired peer must not retain an admission slot merely
    // because its socket is full. Cancellation drops this connection too.
    tokio::select! {
        biased;
        _ = ownership.changed() => Ok(false),
        result = sender.send(message) => {
            result?;
            Ok(true)
        }
    }
}

async fn authenticate_input(
    ws_sender: &mut futures_util::stream::SplitSink<InputSocket, Message>,
    ws_receiver: &mut futures_util::stream::SplitStream<InputSocket>,
    expected: Option<&str>,
    deadline: tokio::time::Instant,
    relaunch: &tokio::sync::Notify,
) -> bool {
    // Authenticate before anything else happens: no greeting, no events.
    if let Some(expected) = expected {
        let first = tokio::time::timeout_at(deadline, ws_receiver.next()).await;
        let ok = match first {
            Ok(Some(Ok(Message::Text(text)))) => matches!(
                serde_json::from_str::<InputEvent>(&text),
                Ok(InputEvent::Auth { token }) if crate::runtime::token_matches(expected, &token)
            ),
            _ => false,
        };
        if !ok {
            // The app reconnects every two seconds; after a handful of these
            // the log has made its point.
            static DROPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = DROPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 5 {
                warn!(
                    "Input client did not authenticate — dropped. Re-sending the token to the app."
                );
            } else if n == 5 {
                warn!("Further unauthenticated clients will be dropped quietly.");
            }
            // Most likely the app was started by hand and never received a
            // token. Launching it again over adb delivers one.
            relaunch.notify_one();
            let _ = tokio::time::timeout_at(deadline, ws_sender.send(Message::Close(None))).await;
            return false;
        }
    }

    true
}

/// Counts of pen actions received, logged periodically. Hover in particular is
/// easy to lose somewhere between the tablet's view hierarchy and here, and
/// without a count there is no way to tell "not sent" from "sent but ignored".
static PEN_ACTIONS: std::sync::Mutex<[u32; 8]> = std::sync::Mutex::new([0; 8]);
static PEN_LOG_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

fn note_pen_action(action: u8) {
    if let Ok(mut c) = PEN_ACTIONS.lock() {
        c[(action as usize).min(7)] += 1;
    }
    let Ok(mut last) = PEN_LOG_AT.lock() else {
        return;
    };
    let now = std::time::Instant::now();
    match *last {
        Some(t) if t.elapsed().as_secs() < 3 => return,
        _ => *last = Some(now),
    }
    if let Ok(mut c) = PEN_ACTIONS.lock() {
        if c.iter().any(|&n| n > 0) {
            info!(
                "Pen events: down={} up={} move={} hover={} hover_exit={} btn_down={} btn_up={}",
                c[0], c[1], c[2], c[3], c[4], c[5], c[6]
            );
            *c = [0; 8];
        }
    }
}

fn handle_event(
    event: InputEvent,
    uinput: &Arc<std::sync::Mutex<InjectDevices>>,
    settings_tx: &Option<watch::Sender<EncoderSettings>>,
    mode_tx: &watch::Sender<bool>,
    latency: &crate::latency::LatencyTracker,
    pen_enabled: bool,
) {
    match event {
        InputEvent::Touch {
            x,
            y,
            pressure,
            action,
            slot,
        } => {
            inject_touch_event(
                uinput,
                AbsoluteContact::from_normalized(x, y, pressure),
                action,
                slot,
            );
        }
        InputEvent::Pen {
            x,
            y,
            pressure,
            tilt_x,
            tilt_y,
            eraser,
            action,
        } => {
            inject_pen_event(
                uinput,
                AbsoluteContact::from_normalized(x, y, pressure),
                (tilt_x, tilt_y),
                eraser,
                action,
                pen_enabled,
            );
        }
        InputEvent::Resolution {
            width,
            height,
            width_mm,
            height_mm,
        } => {
            apply_tablet_resolution(settings_tx, width, height, width_mm, height_mm);
        }
        InputEvent::Rendered { seq, decode_us } => latency.on_rendered(seq, decode_us),
        InputEvent::Config {
            bitrate,
            fps,
            encoder,
        } => {
            apply_tablet_config(settings_tx, bitrate, fps, encoder);
        }
        // Already consumed by handle_connection; a second one is harmless.
        InputEvent::Auth { .. } => {}
        InputEvent::Mode { pen_only } => apply_tablet_mode(mode_tx, pen_only, pen_enabled),
    }
}

struct AbsoluteContact {
    x: i32,
    y: i32,
    pressure: i32,
}
impl AbsoluteContact {
    fn from_normalized(x: f64, y: f64, pressure: f64) -> Self {
        Self {
            x: (x.clamp(0.0, 1.0) * COORD_MAX as f64) as i32,
            y: (y.clamp(0.0, 1.0) * COORD_MAX as f64) as i32,
            pressure: (pressure.clamp(0.0, 1.0) * 4096.0) as i32,
        }
    }
}

fn inject_touch_event(
    devices: &std::sync::Mutex<InjectDevices>,
    contact: AbsoluteContact,
    action: u8,
    slot: u8,
) {
    if let Ok(mut guard) = devices.lock() {
        if let Err(error) = guard.inject_touch(contact.x, contact.y, contact.pressure, action, slot)
        {
            warn!("Failed to inject touch: {}", error);
        }
    }
}

fn inject_pen_event(
    devices: &std::sync::Mutex<InjectDevices>,
    contact: AbsoluteContact,
    tilt: (f64, f64),
    eraser: bool,
    action: u8,
    pen_enabled: bool,
) {
    if pen_enabled {
        note_pen_action(action);
    }
    if let Ok(mut guard) = devices.lock() {
        guard.apply_pen(contact, tilt, eraser, action);
    }
}

impl InjectDevices {
    fn apply_pen(&mut self, contact: AbsoluteContact, tilt: (f64, f64), eraser: bool, action: u8) {
        // Tablet tilt is already in degrees. Convert to integer axis values only.
        let tilt_x = (tilt.0.round() as i32).clamp(-90, 90);
        let tilt_y = (tilt.1.round() as i32).clamp(-90, 90);
        let ok = if let Some(dev) = self.pen.as_mut() {
            match dev.inject_pen(
                contact.x,
                contact.y,
                contact.pressure,
                tilt_x,
                tilt_y,
                action,
                eraser,
            ) {
                Ok(_) => true,
                Err(e) => {
                    warn!("Failed to inject pen: {}", e);
                    false
                }
            }
        } else {
            log_missing_pen(&contact, tilt, eraser, action);
            false
        };
        if ok {
            self.record_pen_state(&contact, action);
        }
    }

    fn record_pen_state(&mut self, contact: &AbsoluteContact, action: u8) {
        if matches!(action, 0..=3) {
            self.last_pen_pos = (contact.x, contact.y);
        }
        // Keep an ordinary cursor at the last pen position when proximity ends.
        if action == 4 {
            self.park_pointer();
        }
        match action {
            0 | 3 => self.pen_proximity = true,
            1 => self.pen_proximity = false,
            4 => {
                self.pen_proximity = false;
                self.pen_button = false;
            }
            5 => self.pen_button = true,
            6 => self.pen_button = false,
            _ => {}
        }
    }

    fn park_pointer(&mut self) {
        let (x, y) = self.last_pen_pos;
        if let Some(dev) = self.pointer.as_mut() {
            let _ = dev.emit(EV_ABS, ABS_X, x);
            let _ = dev.emit(EV_ABS, ABS_Y, y);
            let _ = dev.syn();
        }
    }
}

fn log_missing_pen(contact: &AbsoluteContact, tilt: (f64, f64), eraser: bool, action: u8) {
    match action {
        0 => debug!(
            "Pen DOWN at ({}, {}), eraser={}, tilt=({:.1},{:.1}) — no pen device",
            contact.x, contact.y, eraser, tilt.0, tilt.1
        ),
        1 => debug!("Pen UP   at ({}, {}) — no pen device", contact.x, contact.y),
        _ => {}
    }
}

fn physical_dimensions(width: u32, height: u32) -> (u32, u32) {
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

fn apply_tablet_resolution(
    settings_tx: &Option<watch::Sender<EncoderSettings>>,
    width: u32,
    height: u32,
    width_mm: u32,
    height_mm: u32,
) {
    info!(
        "Tablet reports native resolution: {}x{} ({}x{} mm)",
        width, height, width_mm, height_mm
    );
    let Some(tx) = settings_tx else { return };
    let Some(new) = negotiated_geometry(
        &tx.borrow(),
        (width, height),
        (width_mm, height_mm),
        crate::config::FileConfig::load().auto_resolution,
    ) else {
        return;
    };
    tx.send_if_modified(|current| {
        if *current == new {
            return false;
        }
        *current = new;
        true
    });
}

fn negotiated_geometry(
    current: &EncoderSettings,
    pixels: (u32, u32),
    millimetres: (u32, u32),
    auto_resolution: bool,
) -> Option<EncoderSettings> {
    if !(640..=crate::config::MAX_DIMENSION).contains(&pixels.0)
        || !(480..=crate::config::MAX_DIMENSION).contains(&pixels.1)
    {
        warn!("Ignoring implausible resolution {}x{}", pixels.0, pixels.1);
        return None;
    }
    let mut settings = current.clone();
    if auto_resolution {
        (settings.width, settings.height) = pixels;
    }
    (settings.width_mm, settings.height_mm) = physical_dimensions(millimetres.0, millimetres.1);
    settings.geometry_ready = true;
    Some(settings)
}

fn apply_tablet_config(
    settings_tx: &Option<watch::Sender<EncoderSettings>>,
    bitrate: Option<u32>,
    fps: Option<u32>,
    encoder: Option<String>,
) {
    let Some(tx) = settings_tx else {
        warn!("Received config from tablet but live settings are disabled");
        return;
    };
    let mut new = tx.borrow().clone();
    if let Some(b) = bitrate {
        // Clamped to the same ceiling the config file uses: an
        // unclamped value here would be persisted and poison every
        // later run, which is exactly how installs ended up pinned at
        // 200 Mbps with seconds of queueing delay.
        new.bitrate = b.clamp(
            crate::config::MIN_BITRATE_KBPS,
            crate::config::MAX_BITRATE_KBPS,
        );
        if new.bitrate != b {
            warn!("Tablet asked for {} kbps — clamped to {}", b, new.bitrate);
        }
    }
    if let Some(f) = fps {
        new.fps = f.clamp(crate::config::MIN_FPS, crate::config::MAX_FPS);
    }
    if let Some(e) = encoder {
        if crate::config::supported_encoder(&e) {
            new.encoder = e;
        } else {
            warn!("Ignoring unsupported encoder from tablet: {}", e);
        }
    }
    if *tx.borrow() != new {
        info!(
            "Tablet pushed settings: encoder={} {}kbps @{}fps",
            new.encoder, new.bitrate, new.fps
        );
        let _ = tx.send(new);
    }
}

fn apply_tablet_mode(mode_tx: &watch::Sender<bool>, pen_only: bool, pen_enabled: bool) {
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

#[cfg(test)]
mod tests {
    #[test]
    fn t247_connected_fixture_matches_production_response() {
        let config = super::InputConfig {
            virtual_width: 2960,
            virtual_height: 1848,
            ..Default::default()
        };
        let response = config.response("connected", true, &None);
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../testdata/control-connected.json")).unwrap();
        assert_eq!(serde_json::to_value(response).unwrap(), fixture, "T247");
    }

    #[test]
    fn t145_fallback_preserves_card_ownership() {
        let first = crate::vdisplay::EvdiConnector {
            card: 2,
            name: "DVI-I-1".into(),
            connected: true,
        };
        let second = crate::vdisplay::EvdiConnector {
            card: 3,
            name: "DVI-I-2".into(),
            connected: true,
        };
        let connectors = [first, second];
        assert_eq!(
            super::fallback_output(&connectors, Some(4)),
            None,
            "missing assigned card must not map another tablet"
        );
        assert_eq!(
            super::fallback_output(&connectors, None),
            None,
            "unknown card must not guess between tablets"
        );
        assert_eq!(
            super::fallback_output(&connectors, Some(3)).as_deref(),
            Some("DVI-I-2")
        );
        assert_eq!(
            super::fallback_output(&connectors[..1], None).as_deref(),
            Some("DVI-I-1")
        );
    }

    fn last_input_value(path: &std::path::Path, code: u16) -> i32 {
        let bytes = std::fs::read(path).unwrap();
        bytes
            .as_chunks::<{ std::mem::size_of::<LinuxInputEvent>() }>()
            .0
            .iter()
            .filter(|event| {
                u16::from_ne_bytes(event[18..20].try_into().unwrap()) == code
                    && u16::from_ne_bytes(event[16..18].try_into().unwrap())
                        == if code >= 0x100 { EV_KEY } else { EV_ABS }
            })
            .map(|event| i32::from_ne_bytes(event[20..24].try_into().unwrap()))
            .next_back()
            .unwrap()
    }

    fn input_frames(path: &std::path::Path) -> Vec<Vec<(u16, u16, i32)>> {
        let bytes = std::fs::read(path).unwrap();
        let mut frames = Vec::new();
        let mut frame = Vec::new();
        for event in bytes
            .as_chunks::<{ std::mem::size_of::<LinuxInputEvent>() }>()
            .0
        {
            let kind = u16::from_ne_bytes(event[16..18].try_into().unwrap());
            let code = u16::from_ne_bytes(event[18..20].try_into().unwrap());
            let value = i32::from_ne_bytes(event[20..24].try_into().unwrap());
            if kind == EV_SYN && code == SYN_REPORT {
                frames.push(std::mem::take(&mut frame));
            } else {
                frame.push((kind, code, value));
            }
        }
        assert!(frame.is_empty(), "input frame was not synchronized");
        frames
    }

    #[test]
    fn t317_proximity_exit_releases_stylus_button_for_next_press() {
        for eraser in [false, true] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let mut devices = InjectDevices {
                pen: Some(UInputDevice {
                    file: file.reopen().unwrap(),
                }),
                ..InjectDevices::empty()
            };
            for action in [3, 5, 0, 1] {
                devices.apply_pen(
                    AbsoluteContact {
                        x: 100,
                        y: 200,
                        pressure: 1000,
                    },
                    (0.0, 0.0),
                    eraser,
                    action,
                );
            }
            // Lifting the tip alone must preserve a physically held button.
            assert!(devices.pen_button);
            assert_eq!(last_input_value(file.path(), BTN_STYLUS), 1);
            devices.apply_pen(
                AbsoluteContact {
                    x: 0,
                    y: 0,
                    pressure: 0,
                },
                (0.0, 0.0),
                eraser,
                4,
            );
            assert_eq!(
                last_input_value(file.path(), BTN_STYLUS),
                0,
                "T317: proximity-out left the kernel key state pressed"
            );
            assert!(!devices.pen_button, "T317: stale controller button state");
            assert!(!devices.pen_proximity);
            for action in [3, 5, 6, 4] {
                devices.apply_pen(
                    AbsoluteContact {
                        x: 300,
                        y: 400,
                        pressure: 0,
                    },
                    (0.0, 0.0),
                    eraser,
                    action,
                );
            }
            // Model Linux input_get_disposition's duplicate-key filtering.
            // Both gestures must contain a distinct press and release.
            let mut state = 0;
            let transitions: Vec<_> = input_frames(file.path())
                .into_iter()
                .flatten()
                .filter(|&(kind, code, _)| kind == EV_KEY && code == BTN_STYLUS)
                .filter_map(|(_, _, value)| {
                    if value == state {
                        return None;
                    }
                    state = value;
                    Some(value)
                })
                .collect();
            assert_eq!(transitions, [1, 0, 1, 0], "T317: next click was lost");
            assert!(!devices.pen_button);
        }
    }

    #[test]
    fn t286_release_updates_pen_axes_before_leaving_proximity() {
        for eraser in [false, true] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let mut pen = UInputDevice {
                file: file.reopen().unwrap(),
            };
            pen.inject_pen(100, 200, 1000, 10, 20, 0, eraser).unwrap();
            pen.inject_pen(300, 400, 123, 30, -40, 1, eraser).unwrap();
            let frames = input_frames(file.path());
            let release = frames
                .iter()
                .position(|frame| frame.contains(&(EV_KEY, BTN_TOUCH, 0)))
                .unwrap();
            for (code, value) in [
                (ABS_X, 300),
                (ABS_Y, 400),
                (ABS_TILT_X, 30),
                (ABS_TILT_Y, -40),
                (ABS_PRESSURE, 0),
            ] {
                assert!(
                    frames[release].contains(&(EV_ABS, code, value)),
                    "T286 release lost final axis {code}: {:?}",
                    frames[release]
                );
            }
            let tool = if eraser {
                BTN_TOOL_RUBBER
            } else {
                BTN_TOOL_PEN
            };
            assert!(
                !frames[release].contains(&(EV_KEY, tool, 0)),
                "libinput discards updated axes in a proximity-out frame"
            );
            assert_eq!(frames[release + 1], [(EV_KEY, tool, 0)]);
        }
    }

    #[test]
    fn t286_hover_exit_parks_pointer_at_final_release_position() {
        for eraser in [false, true] {
            let pen = tempfile::NamedTempFile::new().unwrap();
            let pointer = tempfile::NamedTempFile::new().unwrap();
            let mut devices = InjectDevices {
                pen: Some(UInputDevice {
                    file: pen.reopen().unwrap(),
                }),
                pointer: Some(UInputDevice {
                    file: pointer.reopen().unwrap(),
                }),
                ..InjectDevices::empty()
            };
            for (action, x, y) in [(0, 100, 200), (1, 300, 400), (4, 0, 0)] {
                devices.apply_pen(
                    AbsoluteContact {
                        x,
                        y,
                        pressure: 123,
                    },
                    (0.0, 0.0),
                    eraser,
                    action,
                );
            }
            assert_eq!(
                last_input_value(pointer.path(), ABS_X),
                300,
                "T286 stale cursor X"
            );
            assert_eq!(
                last_input_value(pointer.path(), ABS_Y),
                400,
                "T286 stale cursor Y"
            );
            assert!(!devices.pen_proximity);
            assert_eq!(last_input_value(pen.path(), BTN_TOUCH), 0);
            assert_eq!(last_input_value(pen.path(), ABS_PRESSURE), 0);
        }
    }

    #[test]
    fn t146_normalized_input_stays_in_axes_and_releases() {
        for pen in [false, true] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let device = UInputDevice {
                file: file.reopen().unwrap(),
            };
            let mut devices = InjectDevices::empty();
            if pen {
                devices.pen = Some(device);
            } else {
                devices.touch = Some(device);
            }
            let devices = Arc::new(std::sync::Mutex::new(devices));
            let (mode, _rx) = watch::channel(false);
            let tracker = crate::latency::LatencyTracker::new();
            let send = |action, x, y, pressure| {
                let event = if pen {
                    InputEvent::Pen {
                        x,
                        y,
                        pressure,
                        action,
                        tilt_x: 0.0,
                        tilt_y: 0.0,
                        eraser: false,
                    }
                } else {
                    InputEvent::Touch {
                        x,
                        y,
                        pressure,
                        action,
                        slot: 0,
                    }
                };
                handle_event(event, &devices, &None, &mode, &tracker, pen);
            };
            let last = |code| last_input_value(file.path(), code);
            send(0, -0.2, 1.4, 2.0);
            assert_eq!(last(ABS_X), 0, "T146 pen={pen}");
            assert_eq!(last(ABS_Y), COORD_MAX);
            assert_eq!(last(ABS_PRESSURE), 4096);
            send(2, 1.0, 0.0, -0.2);
            assert_eq!(last(ABS_X), COORD_MAX);
            assert_eq!(last(ABS_Y), 0);
            assert_eq!(last(ABS_PRESSURE), 0);
            send(1, 1.5, -0.5, 0.0);
            assert_eq!(last(BTN_TOUCH), 0, "out-of-bounds release must survive");
        }
    }

    #[test]
    fn t086_touch_stays_down_until_last_contact_lifts() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let devices = Arc::new(std::sync::Mutex::new(InjectDevices {
            touch: Some(UInputDevice {
                file: file.reopen().unwrap(),
            }),
            ..InjectDevices::empty()
        }));
        let (mode, _rx) = watch::channel(false);
        let tracker = crate::latency::LatencyTracker::new();
        let send = |slot, action, x, y, pressure| {
            handle_event(
                InputEvent::Touch {
                    slot,
                    action,
                    x,
                    y,
                    pressure,
                },
                &devices,
                &None,
                &mode,
                &tracker,
                false,
            )
        };
        let last = |code| last_input_value(file.path(), code);
        send(0, 0, 0.25, 0.25, 0.5);
        send(1, 0, 0.75, 0.75, 0.75);
        send(0, 1, 0.25, 0.25, 0.0);
        assert_eq!(last(BTN_TOUCH), 1);
        assert_eq!(last(BTN_TOOL_FINGER), 1);
        assert_eq!(last(ABS_PRESSURE), 3072);
        assert_eq!(last(ABS_X), (0.75 * COORD_MAX as f64) as i32);
        send(1, 2, 0.5, 0.5, 0.5);
        assert_eq!(last(ABS_PRESSURE), 2048);
        send(1, 1, 0.5, 0.5, 0.0);
        assert_eq!(last(BTN_TOUCH), 0);
        assert_eq!(last(BTN_TOOL_FINGER), 0);
        assert_eq!(last(ABS_PRESSURE), 0);
    }

    #[tokio::test]
    async fn t092_idle_and_partial_upgrades_expire() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for partial in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (socket, _) = listener.accept().await.unwrap();
            let (mode, _rx) = watch::channel(false);
            let task = tokio::spawn(handle_connection(
                PendingInput::new(socket),
                InputConfig::default(),
                None,
                mode,
                crate::latency::LatencyTracker::new(),
                Arc::new(Controllers::new(Arc::new(std::sync::Mutex::new(
                    InjectDevices::empty(),
                )))),
                Arc::new(tokio::sync::Notify::new()),
            ));
            if partial {
                client.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
            }
            let mut byte = [0];
            let closed = tokio::time::timeout(
                std::time::Duration::from_millis(3500),
                client.read(&mut byte),
            )
            .await;
            assert!(
                matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
                "upgrade outlives accept-to-auth deadline"
            );
            assert!(task.await.unwrap().is_err());
        }
    }

    #[tokio::test]
    async fn t092_pending_connections_are_bounded() {
        use tokio::io::AsyncReadExt;
        let (mode, _mode_rx) = watch::channel(false);
        let (_card_tx, card) = watch::channel(None);
        let (_tablet_tx, tablet) = watch::channel(false);
        let server = InputServer::new(
            InputConfig {
                port: 0,
                touch: false,
                pen: false,
                pointer: false,
                ..InputConfig::default()
            },
            None,
            mode,
            crate::latency::LatencyTracker::new(),
            Arc::new(tokio::sync::Notify::new()),
            card,
            tablet,
        );
        let listener = server.bind().await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { server.run_with_listener(listener).await });
        let mut clients = Vec::new();
        for _ in 0..17 {
            clients.push(tokio::net::TcpStream::connect(addr).await.unwrap());
        }
        let mut byte = [0];
        let closed = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            clients.last_mut().unwrap().read(&mut byte),
        )
        .await;
        task.abort();
        let _ = task.await;
        assert!(
            matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
            "unbounded pending handlers"
        );
    }

    #[tokio::test]
    async fn t323_replacement_cancels_a_backpressured_control_writer() {
        use tokio::io::AsyncWriteExt;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        // Bound both directions so a non-reading peer deterministically fills
        // the server's Pong send buffer without a large memory/network load.
        for stream in [&client, &socket] {
            for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
                let size: libc::c_int = 4096;
                assert_eq!(
                    unsafe {
                        libc::setsockopt(
                            stream.as_raw_fd(),
                            libc::SOL_SOCKET,
                            option,
                            (&size as *const libc::c_int).cast(),
                            std::mem::size_of_val(&size) as libc::socklen_t,
                        )
                    },
                    0
                );
            }
        }
        let controllers = Arc::new(Controllers::new(Arc::new(std::sync::Mutex::new(
            InjectDevices::empty(),
        ))));
        let (mode, _mode_rx) = watch::channel(false);
        let mut task = tokio::spawn(handle_connection(
            PendingInput::new(socket),
            InputConfig::default(),
            None,
            mode,
            crate::latency::LatencyTracker::new(),
            controllers.clone(),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let (mut client, _) = tokio_tungstenite::client_async("ws://localhost/", client)
            .await
            .unwrap();
        assert!(matches!(client.next().await, Some(Ok(Message::Text(_)))));
        // Valid masked 125-byte Ping frames; do not consume the Pong replies.
        let mut ping = vec![0x89, 0xfd, 0, 0, 0, 0];
        ping.extend_from_slice(&[1; 125]);
        let traffic = ping.repeat(4096);
        let stalled = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            client.get_mut().write_all(&traffic),
        )
        .await
        .is_err();
        let _replacement = controllers.claim();
        let retired = tokio::time::timeout(std::time::Duration::from_millis(500), &mut task).await;
        task.abort();
        if retired.is_err() {
            let _ = task.await;
        }
        assert!(stalled, "T323: fixture did not reach socket backpressure");
        assert!(
            matches!(retired, Ok(Ok(Ok(())))),
            "T323: replaced controller retained its blocked writer"
        );
    }

    // T085: a retired socket must not release the replacement controller's contact.
    #[tokio::test]
    async fn t085_reconnect_preserves_current_controller_contacts() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let devices = Arc::new(std::sync::Mutex::new(InjectDevices {
            touch: Some(UInputDevice {
                file: file.reopen().unwrap(),
            }),
            ..InjectDevices::empty()
        }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (mode, _rx) = watch::channel(false);
        let controllers = Arc::new(Controllers::new(devices.clone()));
        let mut clients = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let connection = tokio::spawn(connect_async(format!("ws://{addr}")));
            let (socket, _) = listener.accept().await.unwrap();
            tasks.push(tokio::spawn(handle_connection(
                PendingInput::new(socket),
                InputConfig::default(),
                None,
                mode.clone(),
                crate::latency::LatencyTracker::new(),
                controllers.clone(),
                Arc::new(tokio::sync::Notify::new()),
            )));
            let (mut client, _) = connection.await.unwrap().unwrap();
            response(&mut client).await;
            clients.push(client);
        }
        clients[1]
            .send(Message::Text(
                r#"{"type":"touch","x":0.5,"y":0.5,"pressure":0.5,"action":0,"slot":0}"#.into(),
            ))
            .await
            .unwrap();
        clients[1].send(Message::Ping(vec![1])).await.unwrap();
        assert!(matches!(
            clients[1].next().await,
            Some(Ok(Message::Pong(_)))
        ));
        let _ = clients[0].close(None).await;
        tasks.remove(0).await.unwrap().unwrap();
        assert_eq!(devices.lock().unwrap().active_slots, 1);
        let events = std::fs::read(file.path()).unwrap();
        let keys: Vec<i32> = events
            .as_chunks::<{ std::mem::size_of::<LinuxInputEvent>() }>()
            .0
            .iter()
            .filter_map(|event| {
                let code = u16::from_ne_bytes(event[18..20].try_into().unwrap());
                (code == BTN_TOUCH).then(|| i32::from_ne_bytes(event[20..24].try_into().unwrap()))
            })
            .collect();
        assert_eq!(keys.last(), Some(&1));
        clients[1].close(None).await.unwrap();
        tasks.remove(0).await.unwrap().unwrap();
        assert_eq!(devices.lock().unwrap().active_slots, 0);
    }

    #[tokio::test]
    async fn t109_state_changes_cancel_pending_mapping() {
        for changed in 0..3 {
            let (tablet_tx, mut tablet) = watch::channel(true);
            let (mode_tx, mut mode) = watch::channel(false);
            let (card_tx, mut card) = watch::channel(Some(1));
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let mapping = async move {
                started_tx.send(()).unwrap();
                std::future::pending::<()>().await;
                panic!("stale mapping completed");
            };
            let task = tokio::spawn(async move {
                wait_for_mapping_change(&mut tablet, &mut mode, &mut card, mapping).await
            });
            started_rx.await.unwrap();
            match changed {
                0 => {
                    tablet_tx.send(false).unwrap();
                }
                1 => {
                    mode_tx.send(true).unwrap();
                }
                _ => {
                    card_tx.send(Some(2)).unwrap();
                }
            }
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(100), task)
                    .await
                    .expect("mapping must be interrupted")
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn t084_stopping_server_closes_clients_and_watcher() {
        let (mode_tx, _mode_rx) = watch::channel(false);
        let (_card_tx, card_rx) = watch::channel(None);
        let (tablet_tx, tablet_rx) = watch::channel(false);
        let server = InputServer::new(
            InputConfig {
                port: 0,
                touch: false,
                pen: false,
                pointer: false,
                ..InputConfig::default()
            },
            None,
            mode_tx.clone(),
            crate::latency::LatencyTracker::new(),
            Arc::new(tokio::sync::Notify::new()),
            card_rx,
            tablet_rx,
        );
        let listener = server.bind().await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { server.run_with_listener(listener).await });
        let (mut client, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        response(&mut client).await;
        task.abort();
        let _ = task.await;
        let _ = client.send(Message::Ping(vec![42])).await;
        let next = tokio::time::timeout(std::time::Duration::from_millis(300), client.next()).await;
        assert!(
            !matches!(next, Ok(Some(Ok(Message::Pong(_))))),
            "old controller remains alive"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(tablet_tx.receiver_count(), 0, "watcher survives server");
        assert_eq!(
            mode_tx.receiver_count(),
            1,
            "old session retains mode receiver"
        );
    }

    #[tokio::test]
    async fn t029_x11_maps_only_this_tablets_devices_and_card() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("uscreen-x11-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let xinput = root.join("xinput");
        let xrandr = root.join("xrandr");
        std::fs::write(&xinput, r#"#!/bin/sh
cd "$(dirname "$0")"
if [ "$1" = list ]; then
    printf '%s\n' '↳ UScreen Touch id=10 [slave pointer]' '↳ UScreen Pen Pen (0) id=11 [slave pointer]' '↳ UScreen Touch 2 id=20 [slave pointer]' '↳ UScreen Pen 2 Pen (0) id=21 [slave pointer]' '↳ UScreen Pen 2 Eraser (0) id=22 [slave pointer]' '↳ UScreen Pointer 2 id=23 [slave pointer]'
else
    printf '%s %s %s\n' "$1" "$2" "$3" >> mapped
fi
"#).unwrap();
        std::fs::write(&xrandr, "#!/bin/sh\nprintf '%s\n' 'eDP-1 connected primary 1920x1080+0+0' 'DVI-I-1-1 connected 1920x1080+1920+0' 'DVI-I-2-1 connected 1920x1080+3840+0'\n").unwrap();
        for path in [&xinput, &xrandr] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let connectors = [
            crate::vdisplay::EvdiConnector {
                name: "DVI-I-1".into(),
                card: 8,
                connected: true,
            },
            crate::vdisplay::EvdiConnector {
                name: "DVI-I-2".into(),
                card: 9,
                connected: true,
            },
        ];
        map_devices_using(
            false,
            &DeviceIdentity::for_instance(1),
            Some(9),
            3,
            "x11",
            xinput.to_str().unwrap(),
            xrandr.to_str().unwrap(),
            Some(&connectors),
        )
        .await;
        let mapped = std::fs::read_to_string(root.join("mapped")).unwrap_or_default();
        assert_eq!(
            mapped.lines().collect::<Vec<_>>(),
            [
                "map-to-output 20 DVI-I-2-1",
                "map-to-output 21 DVI-I-2-1",
                "map-to-output 22 DVI-I-2-1",
                "map-to-output 23 DVI-I-2-1"
            ]
        );
        std::fs::remove_file(root.join("mapped")).unwrap();
        map_devices_using(
            true,
            &DeviceIdentity::for_instance(0),
            Some(8),
            2,
            "x11",
            xinput.to_str().unwrap(),
            xrandr.to_str().unwrap(),
            Some(&connectors),
        )
        .await;
        let mapped = std::fs::read_to_string(root.join("mapped")).unwrap();
        assert_eq!(
            mapped.lines().collect::<Vec<_>>(),
            ["map-to-output 10 eDP-1", "map-to-output 11 eDP-1"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    use super::*;
    use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

    fn settings(encoder: &str) -> EncoderSettings {
        EncoderSettings {
            encoder: encoder.into(),
            fps: 60,
            bitrate: 20_000,
            width: 1920,
            height: 1080,
            quality: 18,
            width_mm: 310,
            height_mm: 194,
            stream_scale: 1,
            geometry_ready: true,
        }
    }

    #[test]
    fn t346_greeting_uses_one_encoder_settings_snapshot() {
        let cfg = InputConfig::default();
        let fallback = cfg.response("connected", false, &None);
        assert_eq!(fallback.codec, cfg.codec);
        assert_eq!(fallback.fps, None);
        let (tx, _rx) = watch::channel(settings("h264_nvenc"));
        let source = Some(tx.clone());
        let start = std::sync::Barrier::new(2);
        let mixed = std::thread::scope(|scope| {
            scope.spawn(|| {
                start.wait();
                for i in 0..50_000 {
                    tx.send_modify(|s| {
                        let (encoder, fps) = if i % 2 == 0 {
                            ("hevc_nvenc", 30)
                        } else {
                            ("h264_nvenc", 60)
                        };
                        s.encoder = encoder.into();
                        s.fps = fps;
                    });
                }
            });
            start.wait();
            let mut mixed = 0;
            for _ in 0..50_000 {
                let response = cfg.response("connected", false, &source);
                if !matches!(
                    (response.codec.as_str(), response.fps),
                    ("h264", Some(60)) | ("hevc", Some(30))
                ) {
                    mixed += 1;
                }
            }
            mixed
        });
        assert_eq!(mixed, 0, "T346: greeting mixed codec and FPS revisions");
    }

    async fn connection(
        encoder: &str,
    ) -> (
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        watch::Sender<EncoderSettings>,
        tokio::task::JoinHandle<Result<()>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (settings_tx, _settings_rx) = watch::channel(settings(encoder));
        let tx = settings_tx.clone();
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mode_tx, _rx) = watch::channel(false);
            handle_connection(
                PendingInput::new(socket),
                InputConfig {
                    touch: false,
                    pen: false,
                    ..InputConfig::default()
                },
                Some(tx),
                mode_tx,
                crate::latency::LatencyTracker::new(),
                Arc::new(Controllers::new(Arc::new(std::sync::Mutex::new(
                    InjectDevices::empty(),
                )))),
                Arc::new(tokio::sync::Notify::new()),
            )
            .await
        });
        let (client, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        (client, settings_tx, task)
    }

    async fn response(
        client: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    ) -> serde_json::Value {
        let msg = tokio::time::timeout(std::time::Duration::from_millis(300), client.next())
            .await
            .expect("server must publish settings changes")
            .unwrap()
            .unwrap();
        serde_json::from_str(msg.to_text().unwrap()).unwrap()
    }

    #[test]
    fn t223_fixed_resolution_keeps_pixels_but_negotiates_physical_size() {
        let mut initial = settings("libx264");
        initial.geometry_ready = false;
        for auto in [false, true] {
            let next = negotiated_geometry(&initial, (1280, 800), (220, 138), auto).unwrap();
            assert!(next.geometry_ready);
            assert_eq!((next.width_mm, next.height_mm), (220, 138));
            assert_eq!(
                (next.width, next.height),
                if auto { (1280, 800) } else { (1920, 1080) }
            );
        }
        assert!(negotiated_geometry(&initial, (0, 0), (220, 138), false).is_none());
    }

    #[tokio::test]
    async fn t120_greeting_and_apply_report_effective_frame_rate() {
        let (mut client, tx, task) = connection("libx264").await;
        assert_eq!(response(&mut client).await["fps"], 60);
        let mut next = settings("libx264");
        next.fps = 30;
        tx.send_replace(next);
        assert_eq!(response(&mut client).await["fps"], 30);
        client
            .send(Message::Text(r#"{"type":"config","fps":90}"#.into()))
            .await
            .unwrap();
        assert_eq!(response(&mut client).await["fps"], 90);
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn t063_greeting_reports_live_encoder_codec() {
        let (mut client, _tx, task) = connection("hevc_nvenc").await;
        let greeting = response(&mut client).await;
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(greeting["codec"], "hevc");
    }

    #[tokio::test]
    async fn t063_codec_changes_are_pushed_to_connected_clients() {
        let (mut client, tx, task) = connection("h264_nvenc").await;
        assert_eq!(response(&mut client).await["codec"], "h264");
        tx.send_replace(settings("hevc_nvenc"));
        let update = response(&mut client).await;
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(update["codec"], "hevc");
    }

    #[test]
    fn t063_unknown_encoder_from_tablet_is_rejected() {
        let (tx, rx) = watch::channel(settings("libx264"));
        let (mode_tx, _mode_rx) = watch::channel(false);
        handle_event(
            InputEvent::Config {
                bitrate: None,
                fps: None,
                encoder: Some("unknown".into()),
            },
            &Arc::new(std::sync::Mutex::new(InjectDevices::empty())),
            &Some(tx),
            &mode_tx,
            &crate::latency::LatencyTracker::new(),
            true,
        );
        assert_eq!(rx.borrow().encoder, "libx264");
    }

    #[tokio::test]
    async fn t062_greeting_advertises_disabled_input_devices() {
        let (mut client, _tx, task) = connection("h264_nvenc").await;
        let greeting = response(&mut client).await;
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(greeting["touch"], false);
        assert_eq!(greeting["pen"], false);
    }
}
