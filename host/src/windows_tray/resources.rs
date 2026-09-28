//! Owned Windows resources. Copyright (c) 2026 Geraldo Netto.
use crate::tray_state::{self, State, QUIT, RELEASE, SETTINGS};
use anyhow::Result;
use std::{
    mem::size_of,
    sync::atomic::{AtomicU64, Ordering},
};
use windows_sys::Win32::{
    Foundation::HWND,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub(super) fn check(success: bool) -> Result<()> {
    anyhow::ensure!(success, "Windows tray: {}", std::io::Error::last_os_error());
    Ok(())
}
pub(super) struct Class {
    pub name: Vec<u16>,
}
impl Class {
    pub fn new() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = wide(&format!(
            "BlentTray-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let class = WNDCLASSW {
            lpfnWndProc: Some(super::window::callback),
            hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
            lpszClassName: name.as_ptr(),
            ..Default::default()
        };
        check(unsafe { RegisterClassW(&class) } != 0)?;
        Ok(Self { name })
    }
}
impl Drop for Class {
    fn drop(&mut self) {
        unsafe {
            UnregisterClassW(self.name.as_ptr(), GetModuleHandleW(std::ptr::null()));
        }
    }
}
pub(super) struct Icon(pub HICON);
impl Icon {
    pub fn new() -> Result<Self> {
        let rgba = include_bytes!("../../../packaging/icons/blent-64.rgba");
        let mut dib = Vec::new();
        for value in [40u32, 64, 128, 0x00200001, 0, 16384, 0, 0, 0, 0] {
            dib.extend(value.to_le_bytes());
        }
        for row in rgba.chunks_exact(256).rev() {
            for pixel in row.chunks_exact(4) {
                dib.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
        }
        dib.resize(dib.len() + 512, 0);
        let icon = unsafe {
            CreateIconFromResourceEx(dib.as_ptr(), dib.len() as u32, 1, 0x00030000, 32, 32, 0)
        };
        check(!icon.is_null())?;
        Ok(Self(icon))
    }
}
impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.0);
        }
    }
}
pub(super) struct Menu(pub HMENU);
impl Menu {
    pub fn new(state: State, release: Option<&str>) -> Result<Self> {
        let menu = Self(unsafe { CreatePopupMenu() });
        check(!menu.0.is_null())?;
        menu.append(MF_GRAYED, 0, &state.line())?;
        menu.append(MF_GRAYED, 0, "Display and input unavailable")?;
        menu.append(MF_SEPARATOR, 0, "")?;
        let flags = if state == State::Stopping {
            MF_GRAYED
        } else {
            MF_STRING
        };
        if let Some(label) = tray_state::release_label(release) {
            menu.append(flags, RELEASE, &label)?;
        }
        menu.append(flags, SETTINGS, "Settings")?;
        menu.append(flags, QUIT, "Quit")?;
        Ok(menu)
    }
    fn append(&self, flags: u32, id: usize, text: &str) -> Result<()> {
        check(unsafe { AppendMenuW(self.0, flags, id, wide(text).as_ptr()) } != 0)
    }
}
impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            DestroyMenu(self.0);
        }
    }
}
pub(super) struct Notification {
    pub hwnd: HWND,
    icon: Icon,
}
impl Notification {
    pub fn new(hwnd: HWND) -> Result<Self> {
        Ok(Self {
            hwnd,
            icon: Icon::new()?,
        })
    }
    pub fn data(&self, state: State, release: Option<&str>) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: super::window::EVENT,
            hIcon: self.icon.0,
            ..Default::default()
        };
        for (target, value) in data
            .szTip
            .iter_mut()
            .take(127)
            .zip(tray_state::tooltip(state, release).encode_utf16())
        {
            *target = value;
        }
        data
    }
    pub fn update(&self, state: State, release: Option<&str>, add: bool) -> Result<()> {
        let mut data = self.data(state, release);
        let operation = if add { NIM_ADD } else { NIM_MODIFY };
        check(unsafe { Shell_NotifyIconW(operation, &data) } != 0)?;
        if add {
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            check(unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) } != 0)?;
        }
        unsafe {
            SetWindowTextW(
                self.hwnd,
                wide(&tray_state::tooltip(state, release)).as_ptr(),
            );
        }
        Ok(())
    }
}
impl Drop for Notification {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.data(State::Stopping, None));
        }
    }
}
