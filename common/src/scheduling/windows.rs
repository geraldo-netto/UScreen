//! Windows uses an explicit process class; High is not inherited at creation.
use super::Priority;
use anyhow::{ensure, Result};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetPriorityClass, SetPriorityClass, HIGH_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS,
};

pub(super) fn apply_current(priority: Priority) -> Result<String> {
    apply_process(unsafe { GetCurrentProcess() }, priority)
}

fn apply_process(process: HANDLE, priority: Priority) -> Result<String> {
    let class = match priority {
        Priority::Normal => NORMAL_PRIORITY_CLASS,
        Priority::High => HIGH_PRIORITY_CLASS,
    };
    ensure!(
        unsafe { SetPriorityClass(process, class) } != 0,
        "SetPriorityClass: {}",
        std::io::Error::last_os_error()
    );
    ensure!(
        unsafe { GetPriorityClass(process) } == class,
        "priority class request not effective"
    );
    Ok(format!("Windows process priority class {class:#x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t582_windows_priority_round_trip() {
        use crate::commands::{AsyncCommandExt, SyncCommandExt};
        let role = std::env::var("BLENT_T582_WINDOWS_ROLE").unwrap_or_default();
        if role == "probe" {
            assert_eq!(
                unsafe { GetPriorityClass(GetCurrentProcess()) },
                HIGH_PRIORITY_CLASS
            );
        } else if role == "parent" {
            assert!(apply_current(Priority::High).unwrap().contains("0x80"));
            assert!(fixture("probe").output_bounded().unwrap().status.success());
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let mut child = tokio::process::Command::from(fixture("probe"));
            assert!(runtime
                .block_on(child.output_bounded())
                .unwrap()
                .status
                .success());
            assert!(apply_current(Priority::Normal).unwrap().contains("0x20"));
        } else {
            assert!(fixture("parent").output_bounded().unwrap().status.success());
        }
    }

    fn fixture(role: &str) -> std::process::Command {
        fixture_for(
            "scheduling::windows::tests::t582_windows_priority_round_trip",
            role,
        )
    }

    fn fixture_for(test: &str, role: &str) -> std::process::Command {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", test])
            .env("BLENT_T582_WINDOWS_ROLE", role);
        command
    }

    #[test]
    fn t583_windows_normal_priority_reaches_owned_children() {
        use crate::commands::{AsyncCommandExt, SyncCommandExt};
        const TEST: &str =
            "scheduling::windows::tests::t583_windows_normal_priority_reaches_owned_children";
        match std::env::var("BLENT_T582_WINDOWS_ROLE").as_deref() {
            Ok("normal-probe") => assert_eq!(
                unsafe { GetPriorityClass(GetCurrentProcess()) },
                NORMAL_PRIORITY_CLASS
            ),
            Ok("normal-parent") => {
                apply_current(Priority::High).unwrap();
                apply_current(Priority::Normal).unwrap();
                let output = fixture_for(TEST, "normal-probe").output_bounded().unwrap();
                assert!(output.status.success(), "{output:?}");
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let mut child = tokio::process::Command::from(fixture_for(TEST, "normal-probe"));
                let output = runtime.block_on(child.output_bounded()).unwrap();
                assert!(output.status.success(), "{output:?}");
            }
            _ => {
                let output = fixture_for(TEST, "normal-parent").output_bounded().unwrap();
                assert!(output.status.success(), "{output:?}");
            }
        }
    }

    #[test]
    fn t583_windows_denied_priority_preserves_effective_class() {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let current = unsafe { GetCurrentProcess() };
        let before = unsafe { GetPriorityClass(current) };
        assert_ne!(before, 0);
        let raw =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, GetCurrentProcessId()) };
        assert!(!raw.is_null());
        let query_only = unsafe { OwnedHandle::from_raw_handle(raw) };
        for priority in [Priority::Normal, Priority::High] {
            let error = apply_process(query_only.as_raw_handle(), priority).unwrap_err();
            assert!(error.to_string().contains("SetPriorityClass:"), "{error:#}");
            assert_eq!(unsafe { GetPriorityClass(current) }, before);
        }
    }
}
