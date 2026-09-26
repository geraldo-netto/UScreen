//! "A newer release exists" — nothing more.
//!
//! This deliberately does not download or install anything. On Linux the
//! package manager is the update mechanism, and a daemon that overwrites its
//! own binary behind the package manager's back is how systems end up in
//! states nobody can explain. So the daemon only asks GitHub what the latest
//! tag is, and the tray icon and `blent doctor` say so if it is newer.
//!
//! Uses curl rather than an HTTP client crate: one HTTPS GET a day is not
//! worth a dependency tree, and curl is on every system this runs on.

use blent_config::commands::AsyncCommandExt;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{debug, info};

pub use blent_config::release::PAGE as RELEASES_PAGE;
use blent_config::release::{newer_tag, tag_from_json, API as RELEASES_API};

/// Delay before the first check, so startup is not spent waiting on the
/// network, and the interval between checks after that.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Version string of a newer release, if one exists.
pub type Available = Option<String>;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub use blent_config::version::is_newer;

pub async fn latest_release_tag() -> Option<String> {
    let out = tokio::process::Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "4",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            &format!("User-Agent: blent/{}", current_version()),
            RELEASES_API,
        ])
        .output_bounded()
        .await
        .ok()?;
    if !out.status.success() {
        debug!("update check: curl exited {}", out.status);
        return None;
    }
    tag_from_json(&String::from_utf8_lossy(&out.stdout))
}

/// Runs for the life of the daemon, publishing the newer version (if any) on
/// `tx`. Failures are silent at info level and below: no network is not an
/// error condition for a second-screen daemon.
pub async fn run(tx: watch::Sender<Available>) {
    tokio::time::sleep(FIRST_CHECK).await;
    loop {
        if let Some(tag) = latest_release_tag().await {
            if let Some(latest) = newer_tag(&tag, current_version()) {
                info!(
                    "A newer release is available: {} (running {}). {}",
                    latest,
                    current_version(),
                    RELEASES_PAGE
                );
                let _ = tx.send(Some(latest));
            } else {
                debug!("update check: {} is current", current_version());
                let _ = tx.send(None);
            }
        }
        tokio::time::sleep(INTERVAL).await;
    }
}

#[cfg(test)]
mod coverage_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t374_release_json_contract() {
        for line in include_str!("../../testdata/release-responses.tsv").lines() {
            let parts: Vec<_> = line.split('\t').collect();
            let expected = (parts[1] != "-").then_some(parts[1]);
            assert_eq!(tag_from_json(parts[0]).as_deref(), expected, "T374: {line}");
        }
    }

    #[test]
    fn t123_update_versions_follow_shared_validation_and_precedence() {
        for line in include_str!("../../testdata/version-comparisons.tsv").lines() {
            let parts: Vec<_> = line.split('\t').collect();
            assert_eq!(is_newer(parts[0], parts[1]), parts[2] == "true", "{line}");
        }
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer("1.1.0", "1.0.2"));
        assert!(is_newer("v1.1.0", "1.0.2"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.0.2", "1.0.2"));
        assert!(!is_newer("1.0.1", "1.0.2"));
        assert!(!is_newer("garbage", "1.0.2"));
    }

    #[test]
    fn tag_is_read_from_release_json() {
        let body = r#"{"url":"x","tag_name":"v1.4.0","name":"v1.4.0","assets":[]}"#;
        assert_eq!(tag_from_json(body).as_deref(), Some("v1.4.0"));
        assert_eq!(tag_from_json("not json"), None);
    }
}
