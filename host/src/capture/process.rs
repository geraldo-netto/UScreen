//! Same-user capture process retirement and bounded child termination.
use anyhow::Result;
use std::path::Path;
use tokio::process::Child;
use tracing::{info, warn};

/// SIGTERM first, then reap. The helper installs a SIGTERM handler and uses
/// it to run `evdi_disconnect`; SIGKILL skips that and leaves the connector
/// attached until the kernel gets around to releasing the fd.
pub(super) async fn terminate(child: &mut Child, what: &str) {
    let Some(pid) = child.id() else { return };
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    match tokio::time::timeout(std::time::Duration::from_millis(1500), child.wait()).await {
        Ok(Ok(_)) => info!("{} exited cleanly", what),
        _ => {
            warn!("{} ignored SIGTERM — killing", what);
            let _ = child.start_kill();
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), child.wait()).await;
        }
    }
}

pub(super) async fn retire_orphan_capture(path: &Path) -> Result<()> {
    use uscreen_config::linux::processes;
    let selected: Vec<_> = processes::same_user_processes()?
        .into_iter()
        .filter(|process| process.capture_role(path).is_some())
        .collect();
    let retired = processes::retire(
        &selected,
        std::time::Duration::from_millis(1500),
        std::time::Duration::from_millis(500),
    )
    .await?;
    if retired > 0 {
        warn!(
            "Retired {} stray capture process(es) before starting",
            retired
        );
    }
    Ok(())
}
