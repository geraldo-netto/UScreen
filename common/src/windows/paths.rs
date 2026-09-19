use anyhow::{Context, Result};
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
use windows_sys::{
    core::GUID,
    Win32::{System::Com::CoTaskMemFree, UI::Shell::*},
};

struct Folder(*mut u16);
impl Drop for Folder {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(self.0.cast()) };
    }
}

fn known(id: &GUID) -> Result<PathBuf> {
    let mut folder = Folder(std::ptr::null_mut());
    let result = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut folder.0) };
    anyhow::ensure!(
        result >= 0 && !folder.0.is_null(),
        "Windows known folder unavailable ({result:#x})"
    );
    // SHGetKnownFolderPath owns a NUL-terminated UTF-16 buffer until CoTaskMemFree.
    let mut length = 0;
    unsafe {
        while *folder.0.add(length) != 0 {
            length += 1;
        }
    }
    let path = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(folder.0, length)
    }));
    anyhow::ensure!(path.is_absolute(), "Windows known folder is not absolute");
    Ok(path)
}

pub fn roaming() -> Result<PathBuf> {
    known(&FOLDERID_RoamingAppData).context("locate roaming application data")
}

pub fn local() -> Result<PathBuf> {
    known(&FOLDERID_LocalAppData).context("locate local application data")
}
