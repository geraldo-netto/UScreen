//! T493: an unavailable configuration location must never write into the CWD.
#![cfg(all(target_os = "linux", feature = "storage"))]

#[test]
fn t493_invalid_home_is_an_error_without_relative_writes() {
    if std::env::var_os("USCREEN_T493_LOCATION_CHILD").is_some() {
        assert!(
            uscreen_config::config_path().is_err(),
            "T493: accepted relative config location"
        );
        let store = uscreen_config::storage::ConfigStore::default();
        assert!(store.update(|_| Ok(())).is_err());
        assert!(!std::path::Path::new(".config").exists());
        return;
    }
    for home in [None, Some(""), Some("relative-home")] {
        let root = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "t493_invalid_home_is_an_error_without_relative_writes",
                "--nocapture",
            ])
            .env("USCREEN_T493_LOCATION_CHILD", "1")
            .env("XDG_CONFIG_HOME", "relative-config")
            .env_remove("HOME")
            .current_dir(root.path());
        if let Some(home) = home {
            command.env("HOME", home);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "T493: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
