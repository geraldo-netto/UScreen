//! T493: bind suspended children to an owned job before they can spawn workers.
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
};

pub(super) struct Job(OwnedHandle);

pub(super) fn child_priority(command: &mut std::process::Command, flags: u32) {
    use std::os::windows::process::CommandExt;
    let current = unsafe { GetPriorityClass(GetCurrentProcess()) };
    command.creation_flags(flags | current);
}

fn checked(success: i32) -> io::Result<()> {
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn handle(raw: HANDLE) -> io::Result<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // Each successful Windows call transfers this newly created handle to us.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

impl Job {
    pub(super) fn new() -> io::Result<Self> {
        let owned = handle(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) })?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        checked(unsafe {
            SetInformationJobObject(
                owned.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        })?;
        Ok(Self(owned))
    }

    pub(super) fn start(&self, pid: u32, process: HANDLE) -> io::Result<()> {
        checked(unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) })?;
        resume_child(pid)
    }
}

fn resume_child(pid: u32) -> io::Result<()> {
    let snapshot = handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    checked(unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) })?;
    loop {
        if entry.th32OwnerProcessID == pid {
            return resume_thread(entry.th32ThreadID, pid);
        }
        if unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "suspended child thread disappeared",
            ));
        }
    }
}

fn resume_thread(id: u32, owner: u32) -> io::Result<()> {
    let thread = handle(unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            0,
            id,
        )
    })?;
    if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != owner {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "child thread ownership changed",
        ));
    }
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
