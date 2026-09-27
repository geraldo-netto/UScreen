//! Bounded UTF-16 registry access; handles retain only the requested access.
use anyhow::{ensure, Result};
use windows_sys::Win32::{Foundation::ERROR_FILE_NOT_FOUND, System::Registry::*};

pub(super) struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn checked(status: u32) -> Result<()> {
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32).into());
    }
    Ok(())
}

impl Key {
    pub fn open(path: &str, access: u32) -> Result<Option<Self>> {
        let path = super::native::wide(path.as_ref())?;
        let mut key = std::ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(status)?;
        Ok(Some(Self(key)))
    }
    pub fn create(path: &str) -> Result<Self> {
        let path = super::native::wide(path.as_ref())?;
        let mut key = std::ptr::null_mut();
        checked(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        })?;
        Ok(Self(key))
    }
    pub fn read(&self, name: &str) -> Result<Option<Vec<u16>>> {
        let name = super::native::wide(name.as_ref())?;
        let (mut kind, mut size) = (0, 0);
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(status)?;
        ensure!(
            kind == REG_SZ && size > 0 && size <= 4096 && size % 2 == 0,
            "Invalid autostart registry value"
        );
        let mut value = vec![0u16; size as usize / 2];
        checked(unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                value.as_mut_ptr().cast(),
                &mut size,
            )
        })?;
        value.truncate(size as usize / 2);
        ensure!(
            kind == REG_SZ && size % 2 == 0 && value.pop() == Some(0) && !value.contains(&0),
            "Invalid autostart registry string"
        );
        Ok(Some(value))
    }
    pub fn write(&self, name: &str, value: &[u16]) -> Result<()> {
        ensure!(
            value.len() < 2048 && !value.contains(&0),
            "Invalid registry string"
        );
        let name = super::native::wide(name.as_ref())?;
        let mut value = value.to_vec();
        value.push(0);
        checked(unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        })
    }
    pub fn remove(&self, name: &str) -> Result<()> {
        let name = super::native::wide(name.as_ref())?;
        let status = unsafe { RegDeleteValueW(self.0, name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        checked(status)
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
