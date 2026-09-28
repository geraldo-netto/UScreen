//! T672: read-only Windows monitor inventory. No display creation or mode changes.
use crate::input_mapping::{Monitor, MonitorInventory, Rect, Rotation, Snapshot};
use anyhow::{ensure, Context, Result};
use std::{marker::PhantomData, mem::size_of, ptr, rc::Rc};
use windows_sys::Win32::{
    Foundation::{LPARAM, RECT},
    Graphics::Gdi::*,
    UI::{HiDpi::*, Shell::GetScaleFactorForMonitor},
};

pub struct NativeInventory;
struct Collection {
    monitors: Vec<Monitor>,
    error: Option<anyhow::Error>,
}

impl MonitorInventory for NativeInventory {
    fn snapshot(&self) -> Result<Snapshot> {
        let _context = DpiContext::enter()?;
        let mut collection = Collection {
            monitors: Vec::new(),
            error: None,
        };
        // Callback is synchronous; the context cannot outlive this stack frame.
        let result = unsafe {
            EnumDisplayMonitors(
                ptr::null_mut(),
                ptr::null(),
                Some(collect),
                &mut collection as *mut _ as LPARAM,
            )
        };
        if let Some(error) = collection.error {
            return Err(error);
        }
        ensure!(result != 0, "Windows monitor enumeration failed");
        Snapshot::new(collection.monitors)
    }
}

struct DpiContext {
    previous: DPI_AWARENESS_CONTEXT,
    _thread: PhantomData<Rc<()>>,
}
impl DpiContext {
    fn enter() -> Result<Self> {
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        ensure!(!previous.is_null(), "Per-monitor DPI context unavailable");
        Ok(Self {
            previous,
            _thread: PhantomData,
        })
    }
}
impl Drop for DpiContext {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.previous);
        }
    }
}

unsafe extern "system" fn collect(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
    // Only our synchronous EnumDisplayMonitors call supplies this pointer.
    let collection = &mut *(data as *mut Collection);
    let result = read_monitor(handle).and_then(|monitor| {
        ensure!(
            collection.monitors.len() < 128,
            "Monitor inventory exceeds 128 outputs"
        );
        collection.monitors.push(monitor);
        Ok(())
    });
    match result {
        Ok(()) => 1,
        Err(error) => {
            collection.error = Some(error);
            0
        }
    }
}

fn read_monitor(handle: HMONITOR) -> Result<Monitor> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    ensure!(
        unsafe { GetMonitorInfoW(handle, &mut info.monitorInfo) } != 0,
        "Monitor information unavailable"
    );
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    ensure!(
        unsafe {
            EnumDisplaySettingsExW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode, 0)
        } != 0,
        "Monitor mode unavailable"
    );
    let (bounds, rotation) = geometry(&mode)?;
    let rectangle = info.monitorInfo.rcMonitor;
    ensure!(
        bounds
            == Rect {
                left: rectangle.left,
                top: rectangle.top,
                right: rectangle.right,
                bottom: rectangle.bottom
            },
        "Monitor topology changed during enumeration"
    );
    let mut scale = 0;
    ensure!(
        unsafe { GetScaleFactorForMonitor(handle, &mut scale) } >= 0,
        "Monitor scale unavailable"
    );
    let (id, name) = identity(&info.szDevice)?;
    Ok(Monitor {
        id,
        name,
        bounds,
        rotation,
        scale_percent: scale.try_into()?,
        primary: info.monitorInfo.dwFlags & 1 != 0,
    })
}

fn identity(adapter: &[u16; 32]) -> Result<(String, String)> {
    let mut device = DISPLAY_DEVICEW {
        cb: size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    // EDD_GET_DEVICE_INTERFACE_NAME yields the OS monitor interface identity.
    ensure!(
        unsafe { EnumDisplayDevicesW(adapter.as_ptr(), 0, &mut device, 1) } != 0,
        "Monitor identity unavailable"
    );
    Ok((wide(&device.DeviceID)?, wide(&device.DeviceString)?))
}

fn wide(value: &[u16]) -> Result<String> {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .context("Unterminated native monitor string")?;
    Ok(String::from_utf16(&value[..end])?)
}

fn geometry(mode: &DEVMODEW) -> Result<(Rect, Rotation)> {
    let required = DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYORIENTATION;
    ensure!(
        mode.dmFields & required == required,
        "Monitor mode lacks geometry or rotation"
    );
    // dmFields above identifies the display union variant.
    let position = unsafe { mode.Anonymous1.Anonymous2 };
    // DEVMODE angles are counter-clockwise; shared policy names clockwise angles.
    let rotation = match position.dmDisplayOrientation {
        DMDO_DEFAULT => Rotation::Identity,
        DMDO_90 => Rotation::Clockwise270,
        DMDO_180 => Rotation::Clockwise180,
        DMDO_270 => Rotation::Clockwise90,
        _ => anyhow::bail!("Unsupported monitor rotation"),
    };
    let bounds = Rect {
        left: position.dmPosition.x,
        top: position.dmPosition.y,
        right: position
            .dmPosition
            .x
            .checked_add(mode.dmPelsWidth.try_into()?)
            .context("Monitor width overflow")?,
        bottom: position
            .dmPosition
            .y
            .checked_add(mode.dmPelsHeight.try_into()?)
            .context("Monitor height overflow")?,
    };
    bounds.validate()?;
    Ok((bounds, rotation))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t672_native_inventory_preserves_context_and_observes_monitor_changes() {
        let before = unsafe { GetThreadDpiAwarenessContext() };
        let snapshot = NativeInventory.snapshot().unwrap();
        assert!(
            unsafe { AreDpiAwarenessContextsEqual(before, GetThreadDpiAwarenessContext()) } != 0
        );
        let selected = snapshot.select(&snapshot.monitors()[0].id).unwrap();
        selected.project(&snapshot, 0.5, 0.5).unwrap();
        assert!(selected.current(&NativeInventory.snapshot().unwrap()));
        // Native inventory feeds the same retirement boundary as a native display-change notification.
        let mut changed = snapshot.monitors().to_vec();
        changed[0].bounds.right -= 1;
        let changed = Snapshot::new(changed).unwrap();
        assert!(selected.project(&changed, 0.5, 0.5).is_err());
        let mut collection = Collection {
            monitors: Vec::new(),
            error: None,
        };
        assert_eq!(
            unsafe {
                collect(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut collection as *mut _ as LPARAM,
                )
            },
            0
        );
        assert!(collection.error.is_some());
        assert!(identity(&[0; 32]).is_err());
    }
    #[test]
    fn t672_native_mode_parser_rejects_missing_overflow_and_invalid_fields() {
        let mut mode = DEVMODEW {
            dmFields: DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYORIENTATION,
            dmPelsWidth: 1920,
            dmPelsHeight: 1080,
            ..Default::default()
        };
        for rotation in 0..8 {
            mode.Anonymous1.Anonymous2.dmDisplayOrientation = rotation;
            assert_eq!(geometry(&mode).is_ok(), rotation < 4);
            if let Ok((_, actual)) = geometry(&mode) {
                assert_eq!(
                    actual,
                    [
                        Rotation::Identity,
                        Rotation::Clockwise270,
                        Rotation::Clockwise180,
                        Rotation::Clockwise90
                    ][rotation as usize]
                );
            }
        }
        mode.Anonymous1.Anonymous2.dmDisplayOrientation = 0;
        for width in [0, 1, 1920, i32::MAX as u32, u32::MAX] {
            mode.dmPelsWidth = width;
            assert_eq!(
                geometry(&mode).is_ok(),
                width > 0 && width <= i32::MAX as u32
            );
        }
        mode.dmPelsWidth = 1920;
        mode.Anonymous1.Anonymous2.dmPosition.x = i32::MAX;
        assert!(geometry(&mode).is_err());
        mode.dmFields = 0;
        assert!(geometry(&mode).is_err());
        for value in [&[65, 0][..], &[0][..], &[0xd800, 0][..], &[65][..]] {
            assert_eq!(wide(value).is_ok(), value == [65, 0] || value == [0]);
        }
    }
}
