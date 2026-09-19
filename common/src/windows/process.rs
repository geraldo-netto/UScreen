//! Process identity and retirement through owned handles, never a PID-only kill.
use super::{
    native::{checked, owned},
    security::User,
};
use anyhow::{Context, Result};
use std::{
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, OwnedHandle},
    },
    path::PathBuf,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::*,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Identity {
    pub pid: u32,
    pub started: u64,
    pub executable: PathBuf,
}

fn inspect(handle: HANDLE, pid: u32) -> Result<Identity> {
    anyhow::ensure!(
        unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT,
        "process has exited"
    );
    let owner = User::of(handle)?;
    anyhow::ensure!(
        User::current()?.matches(owner.sid()),
        "process belongs to another user"
    );
    let mut name = vec![0u16; 32768];
    let mut length = name.len() as u32;
    checked(unsafe { QueryFullProcessImageNameW(handle, 0, name.as_mut_ptr(), &mut length) })?;
    let executable = PathBuf::from(std::ffi::OsString::from_wide(&name[..length as usize]));
    let mut times = [FILETIME::default(); 4];
    let [created, exited, kernel, user] = &mut times;
    checked(unsafe { GetProcessTimes(handle, created, exited, kernel, user) })?;
    Ok(Identity {
        pid,
        executable,
        started: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
    })
}

impl Identity {
    pub fn read(pid: u32) -> Result<Self> {
        let process = owned(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        })?;
        inspect(process.as_raw_handle(), pid)
    }

    pub fn is_current(&self) -> bool {
        Self::read(self.pid).is_ok_and(|current| current == *self)
    }

    pub fn retire(&self, timeout: Duration) -> Result<()> {
        anyhow::ensure!(
            self.pid != std::process::id(),
            "refusing to retire the calling process"
        );
        let handle: OwnedHandle = owned(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                self.pid,
            )
        })?;
        anyhow::ensure!(
            inspect(handle.as_raw_handle(), self.pid)? == *self,
            "process identity changed"
        );
        checked(unsafe { TerminateProcess(handle.as_raw_handle(), 1) })
            .context("terminate owned process")?;
        let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        anyhow::ensure!(
            unsafe { WaitForSingleObject(handle.as_raw_handle(), millis) } == WAIT_OBJECT_0,
            "process retirement timed out"
        );
        Ok(())
    }
}
