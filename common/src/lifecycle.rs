//! Shared bounded lifecycle policy; native adapters supply state and operations.
use anyhow::Result;
use std::time::{Duration, Instant};

/// Poll native state without allowing an absent/unresponsive service to hang UI.
pub fn wait_until(timeout: Duration, mut ready: impl FnMut() -> Result<bool>) -> Result<()> {
    let started = Instant::now();
    loop {
        if ready()? {
            return Ok(());
        }
        anyhow::ensure!(started.elapsed() < timeout, "daemon transition timed out");
        std::thread::sleep(
            Duration::from_millis(20).min(timeout.saturating_sub(started.elapsed())),
        );
    }
}

/// A failed stop must never create a second instance.
pub fn restart(
    stop: impl FnOnce() -> Result<(), String>,
    start: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    stop()?;
    start()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t524_transition_bounds_errors_and_restart_order() {
        let mut calls = 0;
        wait_until(Duration::from_secs(1), || {
            calls += 1;
            Ok(calls == 2)
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert!(wait_until(Duration::ZERO, || Ok(false)).is_err());
        assert!(wait_until(Duration::ZERO, || anyhow::bail!("probe failed"))
            .unwrap_err()
            .to_string()
            .contains("probe failed"));
        wait_until(Duration::ZERO, || Ok(true)).unwrap();
        assert!(restart(|| Err("stop failed".into()), || panic!("must not launch")).is_err());
        assert_eq!(
            restart(|| Ok(()), || Err("start failed".into())),
            Err("start failed".into())
        );
        restart(|| Ok(()), || Ok(())).unwrap();
    }
}
