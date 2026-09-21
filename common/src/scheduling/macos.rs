//! Darwin nice is process-wide and inherited by children. Raising priority may
//! be refused without elevated permission; never claim a successful request.
use super::Priority;
use anyhow::{ensure, Result};

pub(super) fn apply_current(priority: Priority) -> Result<String> {
    let nice = match priority {
        Priority::Normal => 0,
        Priority::High => -5,
    };
    ensure!(
        unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) } == 0,
        "setpriority: {}",
        std::io::Error::last_os_error()
    );
    // Reset errno: -1 is a valid nice value, not necessarily an error.
    unsafe {
        *libc::__error() = 0;
    }
    let effective = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
    ensure!(
        unsafe { *libc::__error() } == 0 && effective == nice,
        "nice request not effective"
    );
    Ok(format!(
        "Darwin process nice={effective}; inherited by new children"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t582_darwin_reports_effective_priority_or_permission_denial() {
        let original = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
        let high = apply_current(Priority::High);
        if let Ok(ref result) = high {
            assert!(result.contains("nice=-5"));
        } else {
            assert!(high.unwrap_err().to_string().contains("setpriority"));
        }
        assert!(apply_current(Priority::Normal).unwrap().contains("nice=0"));
        assert_eq!(
            unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, original) },
            0
        );
    }
}
