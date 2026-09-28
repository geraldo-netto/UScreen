//! Shared byte/time bounds and native process-tree ownership.
use anyhow::{ensure, Context, Result};
use blent_config::commands::OwnedChild;
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

pub async fn read(
    program: &std::ffi::OsStr,
    args: &[&str],
    limit: usize,
    deadline: Duration,
) -> Result<Vec<u8>> {
    ensure!(
        (1..=16_777_216).contains(&limit),
        "Invalid command output bound"
    );
    ensure!(!deadline.is_zero(), "Invalid command deadline");
    let operation = async {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = OwnedChild::spawn(&mut command)?;
        let mut bytes = Vec::new();
        child
            .take_stdout()
            .context("Missing command output")?
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(bytes.len() <= limit, "Oversized command output");
        ensure!(
            child.finish(deadline).await?.success(),
            "Owned command failed"
        );
        Ok(bytes)
    };
    tokio::time::timeout(deadline, operation)
        .await
        .context("Owned command deadline")?
}
