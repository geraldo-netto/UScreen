//! Linux process and runtime adapters.
pub mod runtime;
use std::path::PathBuf;

pub fn daemon_is_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let base = PathBuf::from(format!("/proc/{pid}"));
    if std::fs::read_to_string(base.join("comm")).map_or(true, |name| name.trim() != "uscreen") {
        return false;
    }
    // A zombie has finished cleanup but may not yet have been reaped by its
    // parent. Waiting for /proc to disappear would misreport it as running.
    std::fs::read_to_string(base.join("stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, fields)| !fields.starts_with('Z') && !fields.starts_with('X'))
        })
        .unwrap_or(false)
}
