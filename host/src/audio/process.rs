use anyhow::{Context, Result};
use blent_config::audio::{AudioProfile, Direction};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::process::{Child, Command};

pub(super) fn spawn(helper: &Path, profile: AudioProfile, token: &str) -> Result<Child> {
    let direction = match profile.direction {
        Direction::Microphone => "microphone",
        Direction::Speakers => "speakers",
    };
    let child = Command::new(helper)
        .args([
            profile.buffer_ms.to_string(),
            format!("blent_{direction}_{}", &token[..12]),
            direction.to_owned(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("Start native PipeWire audio adapter")?;
    use std::os::fd::AsRawFd;
    if let Some(input) = &child.stdin {
        // Minimize stale kernel backlog; child continuously drains into an age-bounded ring.
        unsafe {
            libc::fcntl(input.as_raw_fd(), libc::F_SETPIPE_SZ, 4096);
        }
    }
    if let Some(output) = &child.stdout {
        unsafe {
            libc::fcntl(output.as_raw_fd(), libc::F_SETPIPE_SZ, 4096);
        }
    }
    Ok(child)
}
pub(super) async fn retire(child: &mut Child) {
    drop(child.stdin.take());
    if tokio::time::timeout(Duration::from_millis(500), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}

pub(super) async fn ready(child: &mut Child) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let mut ready = [0; 6];
    tokio::time::timeout(
        Duration::from_secs(5),
        child
            .stdout
            .as_mut()
            .context("missing adapter status")?
            .read_exact(&mut ready),
    )
    .await??;
    anyhow::ensure!(&ready == b"READY\n", "PipeWire device could not be created");
    Ok(())
}
