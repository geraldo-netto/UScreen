//! A validated owner-only directory, pinned against rename while in use.
use super::{
    native::{checked, wide},
    security::{self, User},
};
use anyhow::{Context, Result};
use std::{
    os::windows::{
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_ALREADY_EXISTS, INVALID_HANDLE_VALUE},
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::*,
};

pub struct Directory {
    path: PathBuf,
    _handle: std::fs::File,
}

impl Directory {
    pub fn create(path: &Path) -> Result<Self> {
        anyhow::ensure!(
            path.is_absolute(),
            "private directory path must be absolute"
        );
        let user = User::current()?;
        let descriptor = security::private_descriptor(&user)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let wide = wide(path.as_os_str())?;
        let created = unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) };
        if created == 0 && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            checked(created).context("create private Windows directory")?;
        }
        let file = open_directory(&wide)?;
        let attributes = file.metadata()?.file_attributes();
        anyhow::ensure!(
            attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "private directory cannot be a reparse point"
        );
        anyhow::ensure!(
            attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
            "private path is not a directory"
        );
        security::validate(file.as_raw_handle(), &user)?;
        Ok(Self {
            path: path.to_owned(),
            _handle: file,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn open_directory(path: &[u16]) -> Result<std::fs::File> {
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    anyhow::ensure!(
        raw != INVALID_HANDLE_VALUE,
        "open private directory: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { std::fs::File::from_raw_handle(raw) })
}
