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
    /// loop — the host timed the frame out, so the round trip needs no clock
    /// agreement between the two devices.
    #[serde(rename = "rendered")]
    Rendered {
        seq: u32,
        /// Microseconds the tablet spent between receiving the frame and
        /// putting it on screen. Subtracting it from the round trip isolates
        /// what the transport actually costs.
        #[serde(default)]
        decode_us: i64,
    },
}

#[derive(Serialize)]
pub struct InputResponse {
    pub status: String,
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
        let codec = settings
            .as_ref()
            .map(|tx| {
                crate::capture::Codec::from_encoder(&tx.borrow().encoder)
                    .muxer()
                    .to_string()
            })
            .unwrap_or_else(|| self.codec.clone());
        InputResponse {
            status: status.into(),
            width: self.virtual_width,
            height: self.virtual_height,
            codec,
            pen_only,
            touch: self.touch,
            pen: self.pen,
        }
    }

    pub fn any_device(&self) -> bool {
        self.touch || self.pen || (self.pen && self.pointer)
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

    fn inject_touch(&mut self, x: i32, y: i32, pressure: i32, action: u8, slot: u8) -> Result<()> {
        match action {
            0 => {
                // DOWN
                self.emit(EV_ABS, ABS_MT_SLOT, slot as i32)?;
                self.emit(EV_ABS, ABS_MT_TRACKING_ID, slot as i32)?;
                self.emit(EV_ABS, ABS_MT_POSITION_X, x)?;
                self.emit(EV_ABS, ABS_MT_POSITION_Y, y)?;
                self.emit(EV_ABS, ABS_MT_PRESSURE, pressure)?;
                self.emit(EV_KEY, BTN_TOUCH, 1)?;
                self.emit(EV_KEY, BTN_TOOL_FINGER, 1)?;
                self.emit(EV_ABS, ABS_X, x)?;
                self.emit(EV_ABS, ABS_Y, y)?;
                self.emit(EV_ABS, ABS_PRESSURE, pressure)?;
                self.syn()?;
            }
            1 => {
                // UP
                self.emit(EV_ABS, ABS_MT_SLOT, slot as i32)?;
                self.emit(EV_ABS, ABS_MT_TRACKING_ID, -1)?;
                self.emit(EV_KEY, BTN_TOUCH, 0)?;
                self.emit(EV_KEY, BTN_TOOL_FINGER, 0)?;
                self.emit(EV_ABS, ABS_PRESSURE, 0)?;
                self.syn()?;
            }
            2 => {
                // MOVE - combine with previous if possible
                self.emit(EV_ABS, ABS_MT_SLOT, slot as i32)?;
                self.emit(EV_ABS, ABS_MT_POSITION_X, x)?;
                self.emit(EV_ABS, ABS_MT_POSITION_Y, y)?;
                self.emit(EV_ABS, ABS_MT_PRESSURE, pressure)?;
                self.emit(EV_ABS, ABS_X, x)?;
                self.emit(EV_ABS, ABS_Y, y)?;
                self.emit(EV_ABS, ABS_PRESSURE, pressure)?;
                self.syn()?;
            }
            _ => {}
        }
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
                // UP
                self.emit(EV_KEY, BTN_TOUCH, 0)?;
                self.emit(EV_KEY, tool, 0)?;
                self.emit(EV_ABS, ABS_PRESSURE, 0)?;
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
    if TOUCH_DEVICES.fetch_add(1, Ordering::SeqCst) == 0 {
        crate::osk::disable().await;
    }
}

async fn osk_touch_device_removed() {
    if TOUCH_DEVICES.fetch_sub(1, Ordering::SeqCst) == 1 {
        crate::osk::restore().await;
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
    /// (DOWN received, no matching UP yet). Bit N → slot N, up to slot 15.
    active_slots: u16,
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
                let _ = dev.syn();
            }
        }
        self.active_slots = 0;

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
            let Some(name) = kwin_device_property(&sysname, "name").await else {
                continue;
            };
            if !ident.owns(&name) {
                continue;
            }
            let ok = crate::kwin::set_property(
                &format!("/org/kde/KWin/InputDevice/{}", sysname),
                KWIN_INPUT_IFACE,
                "outputName",
                "s",
                &output,
            )
            .await;
            match ok {
                true => {
                    // A successful Set is not proof: KWin answers ok and then
                    // keeps the old value when the output is not usable yet.
                    // Only what reads back counts, so a lost mapping is
                    // retried on the next pass instead of logged as done.
                    match kwin_device_property(&sysname, "outputName").await {
                        Some(now) if now == output => {
                            info!("Mapped '{}' ({}) to output {}", name, sysname, output);
                            mapped += 1;
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
        }

        if mapped >= expected {
            return;
        }
    }

    warn!("Input devices did not appear in KWin within 5s — mapping skipped");
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
        let Ok(randr) = tokio::process::Command::new(xrandr)
            .arg("--query")
            .output_bounded()
            .await
        else {
            warn!("X11 input mapping needs xrandr");
            return;
        };
        if !randr.status.success() {
            warn!("xrandr could not query this X11 session");
            return;
        }
        let text = String::from_utf8_lossy(&randr.stdout);
        let active: Vec<(&str, bool)> = text
            .lines()
            .filter_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.get(1) != Some(&"connected") {
                    return None;
                }
                let has_geometry = fields.iter().any(|f| {
                    f.split_once('x').is_some_and(|(w, h)| {
                        w.parse::<u32>().is_ok()
                            && h.split(['+', '-'])
                                .next()
                                .is_some_and(|h| h.parse::<u32>().is_ok())
                            && (h.contains('+') || h.contains('-'))
                    })
                });
                has_geometry.then_some((fields[0], fields.contains(&"primary")))
            })
            .collect();
        let output = if pen_only {
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
                .filter(|(name, _)| {
                    connectors.iter().any(|c| {
                        c.connected
                            && card.is_none_or(|want| c.card == want)
                            && x11_connector_matches(name, &c.name)
                    })
                })
                .map(|(name, _)| *name)
                .collect();
            // Ambiguous names are safer left unmapped than attached to another tablet.
            if candidates.len() == 1 {
                candidates.first().copied()
            } else {
                None
            }
        };
        let Some(output) = output else {
            continue;
        };
        let Ok(devices) = tokio::process::Command::new(xinput)
            .args(["list", "--short"])
            .output_bounded()
            .await
        else {
            warn!("X11 input mapping needs xinput");
            return;
        };
        if !devices.status.success() {
            warn!("xinput could not list input devices");
            return;
        }
        let mut mapped = std::collections::HashSet::new();
        let mut failed = false;
        for line in String::from_utf8_lossy(&devices.stdout).lines() {
            let Some(start) = line.find("UScreen ") else {
                continue;
            };
            let Some((name, rest)) = line[start..].split_once("id=") else {
                continue;
            };
            let Some(kind) = x11_device_kind(name.trim(), ident) else {
                continue;
            };
            let Some(id) = rest
                .split_whitespace()
                .next()
                .filter(|s| s.parse::<u32>().is_ok())
            else {
                continue;
            };
            let ok = tokio::process::Command::new(xinput)
                .args(["map-to-output", id, output])
                .output_bounded()
                .await
                .is_ok_and(|out| out.status.success());
            if ok {
                mapped.insert(kind);
                info!("Mapped '{}' (X11 id {}) to {}", name.trim(), id, output);
            } else {
                failed = true;
            }
        }
        if !failed && mapped.len() >= expected {
            return;
        }
    }
    warn!("X11 output or input devices not ready after 10s; check xrandr providers and xinput");
}

/// The outputs KWin currently knows, as reported by `kscreen-doctor -j`.
/// `None` when the tool is not there at all (not a KDE session).
async fn kscreen_outputs() -> Option<Vec<serde_json::Value>> {
    let out = tokio::process::Command::new("kscreen-doctor")
        .arg("-j")
        .output_bounded()
        .await
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(v.get("outputs")?.as_array()?.clone())
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
            kscreen_outputs().await?;
        } else {
            let connectors = crate::vdisplay::evdi_connectors();
            // This tablet's own card first; "any connected EVDI output" only
            // as a fallback while the card is not known yet.
            let fallback = connectors
                .iter()
                .find(|c| card.is_some_and(|want| c.card == want))
                .or_else(|| connectors.iter().find(|c| c.connected))
                .or_else(|| connectors.first())
                .map(|c| c.name.clone());
            let Some(outputs) = kscreen_outputs().await else {
                return fallback;
            };
            let mine: Vec<&str> = connectors
                .iter()
                .filter(|c| card.is_none_or(|want| c.card == want))
                .map(|c| c.name.as_str())
                .collect();
            let enabled = outputs.iter().find(|o| {
                let name = o.get("name").and_then(|v| v.as_str()).unwrap_or("");
                mine.contains(&name) && o.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false)
            });
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

        // Follow the tablet, the mode and the card for as long as the daemon
        // runs. Attach creates the devices and maps them; detach destroys
        // them; a mode or card switch moves them onto the other output and
        // drops anything held at that moment — a finger or pen tip that was
        // down would otherwise stay down on a screen no longer listening.
        {
            let mut tablet_rx = self.tablet_rx.clone();
            let mut mode_rx = self.mode_tx.subscribe();
            let mut card_rx = self.card_rx.clone();
            let devices = uinput.clone();
            let ident_bg = ident.clone();
            let cfg = self.config.clone();
            tokio::spawn(async move {
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
                        let (c, i) = (cfg.clone(), ident_bg.clone());
                        // Device creation sleeps to let udev settle, so it
                        // runs off the async runtime.
                        let created =
                            tokio::task::spawn_blocking(move || InjectDevices::create(&c, &i))
                                .await
                                .unwrap_or_else(|_| InjectDevices::empty());
                        count = created.count();
                        let has_touch = created.touch.is_some();
                        if let Ok(mut guard) = devices.lock() {
                            *guard = created;
                        }
                        if has_touch {
                            osk_touch_device_added().await;
                        }
                    } else if !attached && present {
                        present = false;
                        let old = devices
                            .lock()
                            .map(|mut g| {
                                g.release_all();
                                std::mem::replace(&mut *g, InjectDevices::empty())
                            })
                            .ok();
                        let had_touch = old.as_ref().is_some_and(|d| d.touch.is_some());
                        drop(old);
                        if had_touch {
                            osk_touch_device_removed().await;
                        }
                        info!("Tablet detached — virtual input devices removed");
                        count = 0;
                    } else if attached {
                        count = devices
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
                    } else {
                        count = 0;
                    }
                    // Leaving display mode tears the virtual output down and
                    // entering it brings the output back. map_devices_to_output
                    // waits for the output it needs to actually be enabled, so
                    // a change during the wait simply restarts it.
                    if count > 0 {
                        map_devices_to_output(pen_only, &ident_bg, card, count).await;
                    }
                    tokio::select! {
                        r = tablet_rx.changed() => { if r.is_err() { break; } }
                        r = mode_rx.changed() => { if r.is_err() { break; } }
                        r = card_rx.changed() => { if r.is_err() { break; } }
                    }
                }
            });
        }

        let config = self.config.clone();
        let running = self.running.clone();

        loop {
            let accept = tokio::select! {
                res = listener.accept() => res,
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

            info!("Input client: {}", peer);
            let cfg = config.clone();
            let settings = self.settings_tx.clone();
            let mode_tx = self.mode_tx.clone();
            let latency = self.latency.clone();
            let devices = uinput.clone();
            let relaunch = self.relaunch.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    handle_connection(socket, cfg, settings, mode_tx, latency, devices, relaunch)
                        .await
                {
                    warn!("Input handler {}: {}", peer, e);
                }
            });
        }

        Ok(())
    }
}

async fn handle_connection(
    raw_stream: tokio::net::TcpStream,
    config: InputConfig,
    settings_tx: Option<watch::Sender<EncoderSettings>>,
    mode_tx: watch::Sender<bool>,
    latency: crate::latency::LatencyTracker,
    uinput: Arc<std::sync::Mutex<InjectDevices>>,
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
    let ws_stream = accept_async_with_config(raw_stream, Some(ws_cfg))
        .await
        .context("WebSocket handshake failed")?;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let mut mode_rx = mode_tx.subscribe();
    let mut settings_rx = settings_tx.as_ref().map(watch::Sender::subscribe);

    // Authenticate before anything else happens: no greeting, no events.
    if let Some(expected) = config.token.as_deref() {
        let first =
            tokio::time::timeout(std::time::Duration::from_secs(3), ws_receiver.next()).await;
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
            let _ = ws_sender.send(Message::Close(None)).await;
            return Ok(());
        }
    }

    let resp = config.response("connected", *mode_rx.borrow_and_update(), &settings_tx);

    ws_sender
        .send(Message::Text(serde_json::to_string(&resp)?))
        .await?;

    loop {
        let msg = tokio::select! {
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
                if ws_sender.send(Message::Text(serde_json::to_string(&response)?)).await.is_err() {
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
                if ws_sender
                    .send(Message::Text(serde_json::to_string(&resp)?))
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            }
        };

        match msg {
            Ok(Message::Text(text)) => match serde_json::from_str::<InputEvent>(&text) {
                Ok(event) => {
                    handle_event(event, &uinput, &settings_tx, &mode_tx, &latency, config.pen);
                }
                Err(e) => {
                    warn!("Invalid input: {} - {}", e, text);
                }
            },
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(Message::Ping(data)) => {
                let _ = ws_sender.send(Message::Pong(data)).await;
            }
            _ => {}
        }
    }

    // Release any stuck MT slots or pen proximity. The devices themselves now
    // outlive the connection, so without this the next client inherits a
    // phantom finger or a pen stuck in proximity — which shows up as
    // unstoppable scrolling.
    if let Ok(mut guard) = uinput.lock() {
        guard.release_all();
    }

    Ok(())
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
            let abs_x = (x * COORD_MAX as f64) as i32;
            let abs_y = (y * COORD_MAX as f64) as i32;
            let abs_pressure = (pressure * 4096.0) as i32;

            if let Ok(mut guard) = uinput.lock() {
                let ok = if let Some(ref mut dev) = guard.touch {
                    match dev.inject_touch(abs_x, abs_y, abs_pressure, action, slot) {
                        Ok(_) => true,
                        Err(e) => {
                            warn!("Failed to inject touch: {}", e);
                            false
                        }
                    }
                } else {
                    match action {
                        0 => debug!("Touch DOWN at ({}, {}) — no touch device", abs_x, abs_y),
                        1 => debug!("Touch UP   at ({}, {}) — no touch device", abs_x, abs_y),
                        _ => {}
                    }
                    false
                };
                if ok {
                    let bit = 1u16 << (slot.min(15) as u16);
                    match action {
                        0 => guard.active_slots |= bit,
                        1 => guard.active_slots &= !bit,
                        _ => {}
                    }
                }
            }
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
            let abs_x = (x * COORD_MAX as f64) as i32;
            let abs_y = (y * COORD_MAX as f64) as i32;
            if pen_enabled {
                note_pen_action(action);
            }
            let abs_pressure = (pressure * 4096.0) as i32;
            // Already degrees, as the tablet computes them. This used to
            // multiply by 180/π on the assumption they were radians, which
            // squashed a pen laid flat at 90° down to 57°.
            let tilt_x_deg = (tilt_x.round() as i32).clamp(-90, 90);
            let tilt_y_deg = (tilt_y.round() as i32).clamp(-90, 90);

            if let Ok(mut guard) = uinput.lock() {
                let ok = if let Some(ref mut dev) = guard.pen {
                    match dev.inject_pen(
                        abs_x,
                        abs_y,
                        abs_pressure,
                        tilt_x_deg,
                        tilt_y_deg,
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
                    match action {
                        0 => debug!(
                            "Pen DOWN at ({}, {}), eraser={}, tilt=({:.1},{:.1}) — no pen device",
                            abs_x, abs_y, eraser, tilt_x, tilt_y
                        ),
                        1 => debug!("Pen UP   at ({}, {}) — no pen device", abs_x, abs_y),
                        _ => {}
                    }
                    false
                };
                if ok {
                    if matches!(action, 0 | 2 | 3) {
                        guard.last_pen_pos = (abs_x, abs_y);
                    }
                    // Leaving proximity hides the tablet cursor, so hand the
                    // position to the plain pointer and let an ordinary cursor
                    // stay where the pen last was.
                    if action == 4 {
                        let (px, py) = guard.last_pen_pos;
                        if let Some(ref mut dev) = guard.pointer {
                            let _ = dev.emit(EV_ABS, ABS_X, px);
                            let _ = dev.emit(EV_ABS, ABS_Y, py);
                            let _ = dev.syn();
                        }
                    }
                    match action {
                        0 | 3 => guard.pen_proximity = true,
                        1 | 4 => guard.pen_proximity = false,
                        5 => guard.pen_button = true,
                        6 => guard.pen_button = false,
                        _ => {}
                    }
                }
            }
        }
        InputEvent::Resolution {
            width,
            height,
            width_mm,
            height_mm,
        } => {
            info!(
                "Tablet reports native resolution: {}x{} ({}x{} mm)",
                width, height, width_mm, height_mm
            );
            let Some(tx) = settings_tx else { return };
            if !crate::config::FileConfig::load().auto_resolution {
                info!("auto_resolution is off — keeping configured resolution");
                return;
            }
            if !(640..=crate::config::MAX_DIMENSION).contains(&width)
                || !(480..=crate::config::MAX_DIMENSION).contains(&height)
            {
                warn!("Ignoring implausible resolution {}x{}", width, height);
                return;
            }
            let mut new = tx.borrow().clone();
            // Reject nonsense physical sizes rather than baking them into an
            // EDID: a bad DPI makes the desktop come up at a absurd scale.
            let (mm_w, mm_h) =
                if (50..=1000).contains(&width_mm) && (50..=1000).contains(&height_mm) {
                    (width_mm, height_mm)
                } else {
                    (
                        crate::edid::DEFAULT_WIDTH_MM,
                        crate::edid::DEFAULT_HEIGHT_MM,
                    )
                };
            if new.width != width
                || new.height != height
                || new.width_mm != mm_w
                || new.height_mm != mm_h
            {
                new.width = width;
                new.height = height;
                new.width_mm = mm_w;
                new.height_mm = mm_h;
                info!(
                    "Auto-resolution: switching virtual display to {}x{} ({}x{} mm)",
                    width, height, mm_w, mm_h
                );
                let _ = tx.send(new);
            }
        }
        InputEvent::Rendered { seq, decode_us } => {
            latency.on_rendered(seq, decode_us);
        }
        InputEvent::Config {
            bitrate,
            fps,
            encoder,
        } => {
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

        // Already consumed by handle_connection; a second one is harmless.
        InputEvent::Auth { .. } => {}

        InputEvent::Mode { pen_only } => {
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
                warn!(
                    "Tablet asked for pen-only mode, but input_pen is off in config.toml — ignored"
                );
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
    }
}

#[cfg(test)]
mod tests {
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
        }
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
                socket,
                InputConfig {
                    touch: false,
                    pen: false,
                    ..InputConfig::default()
                },
                Some(tx),
                mode_tx,
                crate::latency::LatencyTracker::new(),
                Arc::new(std::sync::Mutex::new(InjectDevices::empty())),
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
