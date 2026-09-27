//! Optional per-user login registration. Never writes machine-wide policy.
use super::registry::Key;
use anyhow::{ensure, Context, Result};
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
use windows_sys::Win32::System::Registry::{KEY_QUERY_VALUE, KEY_SET_VALUE};

pub const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "io.github.geraldo_netto.blent";
const SUFFIX: &str = "\" --login";

/// Key selection allows isolated native validation and installer reuse.
pub struct Registration<'a> {
    key: &'a str,
}
impl<'a> Registration<'a> {
    pub fn at(key: &'a str) -> Self {
        Self { key }
    }

    pub fn enabled(&self) -> Result<bool> {
        let Some(key) = Key::open(self.key, KEY_QUERY_VALUE)? else {
            return Ok(false);
        };
        let Some(value) = key.read(VALUE)? else {
            return Ok(false);
        };
        Ok(owned_program(&value).is_some_and(|path| path.is_file()))
    }

    /// Disable removes only a recognized Blent value, even after its executable
    /// disappeared. Enable atomically replaces the same owned value on upgrade.
    pub fn set_enabled(&self, on: bool, binary: Option<&Path>) -> Result<()> {
        let desired = if on {
            Some(command(binary.context("blent binary not found")?)?)
        } else {
            None
        };
        let key = match Key::open(self.key, KEY_QUERY_VALUE | KEY_SET_VALUE)? {
            Some(key) => key,
            None if on => Key::create(self.key)?,
            None => return Ok(()),
        };
        if let Some(existing) = key.read(VALUE)? {
            ensure!(
                owned_program(&existing).is_some(),
                "Autostart value is not owned by Blent; left unchanged"
            );
        }
        match desired {
            Some(value) => key.write(VALUE, &value),
            None => key.remove(VALUE),
        }
    }
}

fn owned_program(value: &[u16]) -> Option<PathBuf> {
    if value.len() > 260 {
        return None;
    }
    let suffix: Vec<_> = SUFFIX.encode_utf16().collect();
    let path = value
        .strip_prefix(&[b'"' as u16])?
        .strip_suffix(suffix.as_slice())?;
    if path.iter().any(|c| *c == 0 || *c == b'"' as u16) {
        return None;
    }
    let path = PathBuf::from(OsString::from_wide(path));
    (path.is_absolute() && path.file_name()?.eq_ignore_ascii_case("blent.exe")).then_some(path)
}

fn command(binary: &Path) -> Result<Vec<u16>> {
    let mut command = vec![b'"' as u16];
    command.extend(binary.as_os_str().encode_wide());
    command.extend(SUFFIX.encode_utf16());
    ensure!(
        owned_program(&command).as_deref() == Some(binary) && binary.is_file(),
        "Autostart requires an existing absolute blent.exe path"
    );
    // Run values have a documented 260-character command-line limit.
    ensure!(
        command.len() <= 260,
        "Autostart command exceeds Windows Run's 260-character limit"
    );
    Ok(command)
}

#[cfg(test)]
#[path = "autostart_tests.rs"]
mod tests;
