//! Windows uses an explicit process class; High is not inherited at creation.
use super::Priority;
use anyhow::{ensure, Result};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetPriorityClass, SetPriorityClass, HIGH_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS,
};

pub(super) fn apply_current(priority: Priority) -> Result<String> {
    let class = match priority {
        Priority::Normal => NORMAL_PRIORITY_CLASS,
        Priority::High => HIGH_PRIORITY_CLASS,
    };
    let process = unsafe { GetCurrentProcess() };
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
        let role = std::env::var("USCREEN_T582_WINDOWS_ROLE").unwrap_or_default();
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
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "scheduling::windows::tests::t582_windows_priority_round_trip",
            ])
            .env("USCREEN_T582_WINDOWS_ROLE", role);
        command
    }
}
