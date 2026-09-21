//! Linux virtual-device lifetime, coordinate conversion and event emission.
use super::backend::{InputBackend, InputSink, PenSample};
#[cfg(test)]
pub(super) use super::event_writer::LinuxInputEvent;
use super::mapping::map_devices_to_output;
use super::InputConfig;
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::{atomic::Ordering, Arc};
use tokio::sync::watch;
use tracing::{debug, info, warn};

pub struct Backend {
    devices: Arc<std::sync::Mutex<InjectDevices>>,
    card: watch::Receiver<Option<u32>>,
}

impl Backend {
    pub fn new(card: watch::Receiver<Option<u32>>) -> Self {
        Self {
            devices: Arc::new(std::sync::Mutex::new(InjectDevices::empty())),
            card,
        }
    }
}

impl InputBackend for Backend {
    fn sink(&self) -> Arc<dyn InputSink> {
        self.devices.clone()
    }
    fn follow(
        &self,
        tablet: watch::Receiver<bool>,
        mode: watch::Receiver<bool>,
        config: InputConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(follow_input_devices(
            tablet,
            mode,
            self.card.clone(),
            self.devices.clone(),
            DeviceIdentity::for_instance(config.instance),
            config,
        ))
    }
}

impl InputSink for std::sync::Mutex<InjectDevices> {
    fn release_all(&self) {
        if let Ok(mut devices) = self.lock() {
            devices.release_all();
        }
    }
    fn touch(&self, position: (f64, f64, f64), action: u8, slot: u8) {
        inject_touch_event(
            self,
            AbsoluteContact::from_normalized(position.0, position.1, position.2),
            action,
            slot,
        );
    }
    fn pen(&self, sample: PenSample, enabled: bool) {
        let (x, y, pressure) = sample.position;
        inject_pen_event(
            self,
            AbsoluteContact::from_normalized(x, y, pressure),
            sample.tilt,
            sample.eraser,
            sample.action,
            sample.button,
            enabled,
        );
    }
}

// Linux input event constants
pub(super) const EV_SYN: u16 = 0x00;
pub(super) const EV_KEY: u16 = 0x01;
pub(super) const EV_ABS: u16 = 0x03;

pub(super) const SYN_REPORT: u16 = 0x00;

pub(super) const BTN_TOUCH: u16 = 0x14a;
pub(super) const BTN_TOOL_FINGER: u16 = 0x145;
pub(super) const BTN_TOOL_PEN: u16 = 0x140;
pub(super) const BTN_TOOL_RUBBER: u16 = 0x141;

pub(super) const ABS_X: u16 = 0x00;
pub(super) const ABS_Y: u16 = 0x01;
pub(super) const ABS_PRESSURE: u16 = 0x18;
pub(super) const ABS_MT_SLOT: u16 = 0x2f;
pub(super) const ABS_MT_POSITION_X: u16 = 0x35;
pub(super) const ABS_MT_POSITION_Y: u16 = 0x36;
pub(super) const ABS_MT_TRACKING_ID: u16 = 0x39;
pub(super) const ABS_MT_PRESSURE: u16 = 0x3a;
pub(super) const ABS_TILT_X: u16 = 0x1a;
pub(super) const ABS_TILT_Y: u16 = 0x1b;

pub(super) const BTN_STYLUS: u16 = 0x14b;
pub(super) const BTN_LEFT: u16 = 0x110;

// uinput ioctl constants (modern UI_DEV_SETUP/UI_ABS_SETUP API — the legacy
// uinput_user_dev write() API cannot declare axis resolution, which makes
// libinput reject the device: "missing tablet capabilities ... resolution")
pub(super) const UI_SET_EVBIT: libc::c_ulong = 0x40045564;
pub(super) const UI_SET_KEYBIT: libc::c_ulong = 0x40045565;
pub(super) const UI_SET_ABSBIT: libc::c_ulong = 0x40045567;
pub(super) const UI_SET_PROPBIT: libc::c_ulong = 0x4004556e;
pub(super) const UI_DEV_SETUP: libc::c_ulong = 0x405c5503;
pub(super) const UI_ABS_SETUP: libc::c_ulong = 0x401c5504;
pub(super) const UI_DEV_CREATE: libc::c_ulong = 0x5501;
pub(super) const UI_DEV_DESTROY: libc::c_ulong = 0x5502;

pub(super) const BUS_VIRTUAL: u16 = 0x06;
pub(super) const INPUT_PROP_DIRECT: i32 = 0x01;

/// Coordinates are injected in a fixed 0..65535 space — the compositor maps
/// the device onto the output, so the virtual display resolution can change
/// at runtime without recreating uinput devices.
pub(super) const COORD_MAX: i32 = 65535;

/// Identity of the virtual input devices. These must stay stable: KDE keys the
/// device→output association in kcminputrc on vendor/product/name, so changing
/// any of them silently orphans the mapping written by `map_devices_to_output`.
pub(super) const UINPUT_VENDOR: u16 = 0x4553;
pub(super) const PRODUCT_TOUCH: u16 = 0x0001;
pub(super) const PRODUCT_PEN: u16 = 0x0002;
pub(super) const PRODUCT_POINTER: u16 = 0x0003;
pub(super) const TOUCH_DEVICE_NAME: &str = "UScreen Touch";
pub(super) const PEN_DEVICE_NAME: &str = "UScreen Pen";
pub(super) const POINTER_DEVICE_NAME: &str = "UScreen Pointer";

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
    pub(super) fn owns(&self, name: &str) -> bool {
        name == self.touch || name == self.pen || name == self.pointer
    }
}
/// ~310mm wide active area → 65535/310 ≈ 211 units/mm.
/// libinput requires a resolution on touchscreen/tablet axes.
pub(super) const RESOLUTION_UNITS_PER_MM: i32 = 211;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct InputAbsInfo {
    pub(super) value: i32,
    pub(super) minimum: i32,
    pub(super) maximum: i32,
    pub(super) fuzz: i32,
    pub(super) flat: i32,
    pub(super) resolution: i32,
}

// Linux/libinput angular resolution is units per radian. Milliradians keep
// wire degrees within 0.03 degrees after integer quantization (T287).
pub(super) const TILT_UNITS_PER_RADIAN: i32 = 1000;

pub(super) fn tilt_axis_units(degrees: f64) -> i32 {
    (degrees.clamp(-90.0, 90.0).to_radians() * f64::from(TILT_UNITS_PER_RADIAN)).round() as i32
}

pub(super) fn pen_tilt_info() -> InputAbsInfo {
    InputAbsInfo {
        minimum: tilt_axis_units(-90.0),
        maximum: tilt_axis_units(90.0),
        resolution: TILT_UNITS_PER_RADIAN,
        ..Default::default()
    }
}

#[repr(C)]
pub(super) struct UinputAbsSetup {
    pub(super) code: u16,
    pub(super) _pad: u16,
    pub(super) absinfo: InputAbsInfo,
}

#[repr(C)]
pub(super) struct UinputSetup {
    pub(super) bustype: u16,
    pub(super) vendor: u16,
    pub(super) product: u16,
    pub(super) version: u16,
    pub(super) name: [u8; 80],
    pub(super) ff_effects_max: u32,
}

/// A uinput virtual input device for injecting touch/pen events into Linux.
/// Touch and pen are SEPARATE devices: libinput classifies a touchscreen and
/// a tablet pen differently and rejects a device that mixes both.
pub(super) struct UInputDevice<W: Write + AsRawFd = File> {
    pub(super) file: W,
    batch: super::event_writer::EventBatch,
}

impl UInputDevice {
    pub(super) fn open_uinput() -> Result<File> {
        OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .context("Failed to open /dev/uinput. Ensure the uinput module is loaded and you have permissions (try: sudo modprobe uinput)")
    }

    /// Touchscreen device: multitouch + single-touch axes, INPUT_PROP_DIRECT.
    pub(super) fn new_touch(name: &str, product: u16) -> Result<Self> {
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
        Ok(Self::from_writer(file))
    }

    /// Pen tablet device: stylus tool + pressure + tilt.
    /// No INPUT_PROP_DIRECT — that flag means "touchscreen" and causes KDE to
    /// activate the on-screen keyboard on every pen tap. Without it, libinput
    /// classifies this as a tablet tool (Wacom-style): the cursor follows the
    /// pen position and clicks work as mouse clicks.
    pub(super) fn new_pen(name: &str, product: u16) -> Result<Self> {
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
            Self::abs_setup_info(fd, ABS_TILT_X, pen_tilt_info())?;
            Self::abs_setup_info(fd, ABS_TILT_Y, pen_tilt_info())?;

            Self::dev_setup_and_create(fd, name, product)?;
        }

        info!("uinput pen tablet '{}' created", name);
        std::thread::sleep(std::time::Duration::from_millis(200));
        Ok(Self::from_writer(file))
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
    pub(super) fn new_pointer(name: &str, product: u16) -> Result<Self> {
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
        Ok(Self::from_writer(file))
    }

    pub(super) unsafe fn abs_setup(
        fd: i32,
        code: u16,
        min: i32,
        max: i32,
        resolution: i32,
    ) -> Result<()> {
        Self::abs_setup_info(
            fd,
            code,
            InputAbsInfo {
                minimum: min,
                maximum: max,
                resolution,
                ..Default::default()
            },
        )
    }

    pub(super) unsafe fn abs_setup_info(fd: i32, code: u16, absinfo: InputAbsInfo) -> Result<()> {
        let setup = UinputAbsSetup {
            code,
            _pad: 0,
            absinfo,
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

    pub(super) unsafe fn dev_setup_and_create(fd: i32, name: &str, product: u16) -> Result<()> {
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

    pub(super) unsafe fn ioctl_val(fd: i32, request: libc::c_ulong, value: i32) -> Result<()> {
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
}

impl<W: Write + AsRawFd> UInputDevice<W> {
    pub(super) fn from_writer(file: W) -> Self {
        Self {
            file,
            batch: Default::default(),
        }
    }
    pub(super) fn emit(&mut self, type_: u16, code: u16, value: i32) -> Result<()> {
        self.batch.push(type_, code, value)?;
        Ok(())
    }
    pub(super) fn syn(&mut self) -> Result<()> {
        self.batch.finish(&mut self.file)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn inject_pen(
        &mut self,
        x: i32,
        y: i32,
        pressure: i32,
        tilt_x: i32,
        tilt_y: i32,
        action: u8,
        eraser: bool,
        button: Option<bool>,
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
                // DOWN: proximity first, changed button state if any, then tip.
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

                self.inject_pen_button(button)?;
                self.emit(EV_KEY, BTN_TOUCH, 1)?;
                self.emit(EV_ABS, ABS_PRESSURE, pressure)?;
                self.syn()?;
            }
            1 => {
                // UP lifts the tip but keeps the tool and held button in range.
                // Only explicit exit/cancel or controller teardown ends proximity.
                self.inject_pen_button(button)?;
                self.emit(EV_ABS, ABS_X, x)?;
                self.emit(EV_ABS, ABS_Y, y)?;
                self.emit(EV_ABS, ABS_TILT_X, tilt_x)?;
                self.emit(EV_ABS, ABS_TILT_Y, tilt_y)?;
                self.emit(EV_KEY, BTN_TOUCH, 0)?;
                self.emit(EV_ABS, ABS_PRESSURE, 0)?;
                self.syn()?;
            }
            2 => {
                // MOVE (pressing)
                self.inject_pen_button(button)?;
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
                self.inject_pen_button(button)?;
            }
            4 => {
                // Explicit exit/cancel from the Android input surface.
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

    pub(super) fn inject_pen_button(&mut self, button: Option<bool>) -> Result<()> {
        if let Some(down) = button {
            self.emit(EV_KEY, BTN_STYLUS, i32::from(down))?;
            self.syn()?;
        }
        Ok(())
    }
}

impl<W: Write + AsRawFd> Drop for UInputDevice<W> {
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
pub(super) fn create_device(
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
pub(super) static TOUCH_DEVICES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub(super) async fn osk_touch_device_added() {
    TOUCH_DEVICES.fetch_add(1, Ordering::SeqCst);
    crate::osk::sync_touch_state(&TOUCH_DEVICES).await;
}

pub(super) async fn osk_touch_device_removed() {
    TOUCH_DEVICES.fetch_sub(1, Ordering::SeqCst);
    crate::osk::sync_touch_state(&TOUCH_DEVICES).await;
}

/// Own device lifetime even when the watcher is cancelled inside an await.
pub(super) struct DeviceOwner {
    pub(super) devices: Arc<std::sync::Mutex<InjectDevices>>,
    pub(super) touch_registered: bool,
}
impl DeviceOwner {
    pub(super) async fn create_devices(
        &mut self,
        cfg: &InputConfig,
        ident: &DeviceIdentity,
    ) -> usize {
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

    pub(super) async fn remove_devices(&mut self) {
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

    pub(super) fn release_for_remap(&self, pen_only: bool, card: Option<u32>) -> usize {
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
pub(super) struct InjectDevices<W: Write + AsRawFd = File> {
    pub(super) touch: Option<UInputDevice<W>>,
    pub(super) pen: Option<UInputDevice<W>>,
    /// Takes over the cursor when the pen leaves proximity, so it stays where
    /// the user last pointed instead of vanishing.
    pub(super) pointer: Option<UInputDevice<W>>,
    pub(super) last_pen_pos: (i32, i32),
    /// Bitmask of MT slots that currently have an active tracking ID
    /// (DOWN received, no matching UP yet). Bit N identifies slot N, 0–9.
    pub(super) active_slots: u16,
    pub(super) touch_contacts: [Option<(i32, i32, i32)>; 10],
    pub(super) pen_proximity: bool,
    /// S Pen side button currently held (BTN_STYLUS). Tracked so a held
    /// button is released cleanly if the connection drops.
    pub(super) pen_button: bool,
}

impl<W: Write + AsRawFd> InjectDevices<W> {
    pub(super) fn empty() -> Self {
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

    pub(super) fn count(&self) -> usize {
        self.touch.is_some() as usize
            + self.pen.is_some() as usize
            + self.pointer.is_some() as usize
    }

    pub(super) fn inject_touch(
        &mut self,
        x: i32,
        y: i32,
        pressure: i32,
        action: u8,
        slot: u8,
    ) -> Result<()> {
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
    pub(super) fn release_all(&mut self) {
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

pub(super) const KWIN_INPUT_IFACE: &str = "org.kde.KWin.InputDevice";

/// Prefer the enabled primary physical output; otherwise use the first enabled
/// physical output in the inventory. Pen-only mode drives this host screen,
/// never another tablet's EVDI connector.
/// Counts of pen actions received, logged periodically. Hover in particular is
/// easy to lose somewhere between the tablet's view hierarchy and here, and
/// without a count there is no way to tell "not sent" from "sent but ignored".
pub(super) static PEN_ACTIONS: std::sync::Mutex<[u32; 8]> = std::sync::Mutex::new([0; 8]);
pub(super) static PEN_LOG_AT: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);

pub(super) fn note_pen_action(action: u8) {
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

pub(super) struct AbsoluteContact {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) pressure: i32,
}
impl AbsoluteContact {
    pub(super) fn from_normalized(x: f64, y: f64, pressure: f64) -> Self {
        Self {
            x: (x.clamp(0.0, 1.0) * COORD_MAX as f64) as i32,
            y: (y.clamp(0.0, 1.0) * COORD_MAX as f64) as i32,
            pressure: (pressure.clamp(0.0, 1.0) * 4096.0) as i32,
        }
    }
}

pub(super) fn inject_touch_event(
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

pub(super) fn inject_pen_event(
    devices: &std::sync::Mutex<InjectDevices>,
    contact: AbsoluteContact,
    tilt: (f64, f64),
    eraser: bool,
    action: u8,
    button: Option<bool>,
    pen_enabled: bool,
) {
    if pen_enabled {
        note_pen_action(action);
    }
    if let Ok(mut guard) = devices.lock() {
        guard.apply_pen(contact, tilt, eraser, action, button);
    }
}

impl<W: Write + AsRawFd> InjectDevices<W> {
    pub(super) fn apply_pen(
        &mut self,
        contact: AbsoluteContact,
        tilt: (f64, f64),
        eraser: bool,
        action: u8,
        button: Option<bool>,
    ) {
        // Position samples restore actual button state after Android hover exits.
        // Avoid emitting duplicate key/SYN frames for unchanged historical samples.
        let button = button.filter(|down| action <= 3 && *down != self.pen_button);
        // Preserve wire degrees; advertise and emit Linux milliradians.
        let tilt_x = tilt_axis_units(tilt.0);
        let tilt_y = tilt_axis_units(tilt.1);
        let ok = if let Some(dev) = self.pen.as_mut() {
            match dev.inject_pen(
                contact.x,
                contact.y,
                contact.pressure,
                tilt_x,
                tilt_y,
                action,
                eraser,
                button,
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
            self.record_pen_state(&contact, action, button);
        }
    }

    pub(super) fn record_pen_state(
        &mut self,
        contact: &AbsoluteContact,
        action: u8,
        button: Option<bool>,
    ) {
        if matches!(action, 0..=3) {
            self.last_pen_pos = (contact.x, contact.y);
        }
        if let Some(down) = button {
            self.pen_button = down;
        }
        // Keep an ordinary cursor at the last pen position when proximity ends.
        if action == 4 {
            self.park_pointer();
        }
        match action {
            0 | 3 => self.pen_proximity = true,
            4 => {
                self.pen_proximity = false;
                self.pen_button = false;
            }
            5 => self.pen_button = true,
            6 => self.pen_button = false,
            _ => {}
        }
    }

    pub(super) fn park_pointer(&mut self) {
        let (x, y) = self.last_pen_pos;
        if let Some(dev) = self.pointer.as_mut() {
            let _ = dev.emit(EV_ABS, ABS_X, x);
            let _ = dev.emit(EV_ABS, ABS_Y, y);
            let _ = dev.syn();
        }
    }
}

pub(super) fn log_missing_pen(
    contact: &AbsoluteContact,
    tilt: (f64, f64),
    eraser: bool,
    action: u8,
) {
    match action {
        0 => debug!(
            "Pen DOWN at ({}, {}), eraser={}, tilt=({:.1},{:.1}) — no pen device",
            contact.x, contact.y, eraser, tilt.0, tilt.1
        ),
        1 => debug!("Pen UP   at ({}, {}) — no pen device", contact.x, contact.y),
        _ => {}
    }
}

pub(super) async fn follow_input_devices(
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

pub(super) async fn wait_for_mapping_change(
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

impl InjectDevices<File> {
    /// Creates whichever devices the config asks for. The pointer only
    /// exists to park the cursor when the pen lifts, so it is tied to the
    /// pen here and nowhere else has to know that rule.
    pub(super) fn create(cfg: &InputConfig, ident: &DeviceIdentity) -> Self {
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
}

#[cfg(test)]
#[path = "linux/coverage_tests.rs"]
mod coverage_tests;
