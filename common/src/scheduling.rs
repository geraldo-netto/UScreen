//! Portable scheduling preference. Native adapters report effective results;
//! a refused request must never prevent display startup.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Normal,
    #[default]
    High,
}

#[cfg(all(feature = "platform", target_os = "linux"))]
mod linux;
#[cfg(all(feature = "commands", target_os = "macos"))]
mod macos;
#[cfg(all(feature = "platform", windows))]
mod windows;

#[cfg(any(feature = "platform", all(feature = "commands", target_os = "macos")))]
pub fn apply_current(priority: Priority) -> anyhow::Result<String> {
    #[cfg(target_os = "linux")]
    return linux::apply_current(priority);
    #[cfg(windows)]
    return windows::apply_current(priority);
    #[cfg(target_os = "macos")]
    return macos::apply_current(priority);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    anyhow::bail!("scheduling backend unavailable for {:?}", priority)
}

#[cfg(feature = "platform")]
pub fn apply_configured() -> String {
    let priority = crate::FileConfig::load().scheduling_priority;
    match apply_current(priority) {
        Ok(effective) => {
            eprintln!("Blent scheduling: {priority:?}; {effective}");
            format!("This process: {priority:?} priority active")
        }
        Err(error) => {
            eprintln!("Blent scheduling: {priority:?} request unavailable: {error:#}; continuing");
            format!("This process: {priority:?} priority unavailable; using OS settings")
        }
    }
}

#[cfg(all(feature = "platform", target_os = "linux"))]
pub fn apply_shared_adb(priority: Priority) {
    if let Err(error) = linux::apply_shared_adb(priority) {
        tracing::warn!("Shared ADB scheduling request unavailable: {error:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t582_high_default_and_normal_override_round_trip_and_merge() {
        let baseline: crate::FileConfig = toml::from_str("fps = 30").unwrap();
        assert_eq!(baseline.scheduling_priority, Priority::High);
        let mut edited = baseline.clone();
        edited.scheduling_priority = Priority::Normal;
        let encoded = toml::to_string(&edited).unwrap();
        assert_eq!(
            toml::from_str::<crate::FileConfig>(&encoded).unwrap(),
            edited
        );
        let merged = edited.merge_edits(&baseline, baseline.clone()).unwrap();
        assert_eq!(merged.scheduling_priority, Priority::Normal);
        assert!(edited.requires_restart_from(&baseline));
        for value in ["realtime", "-20", "", "HIGH", "fifo"] {
            let text = format!("scheduling_priority = {value:?}");
            assert!(toml::from_str::<crate::FileConfig>(&text).is_err());
        }
    }
}
