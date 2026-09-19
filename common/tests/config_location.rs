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
        assert_eq!(
            serde_json::to_value(store.load()).unwrap(),
            serde_json::to_value(uscreen_config::FileConfig::default()).unwrap()
        );
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

#[cfg(feature = "platform")]
#[test]
fn t497_default_adapters_publish_only_the_isolated_current_preference() {
    if std::env::var_os("USCREEN_T497_CONFIG_CHILD").is_some() {
        let baseline = uscreen_config::FileConfig::default();
        let edited = uscreen_config::FileConfig {
            pipe_capacity_mib: 8,
            ..baseline.clone()
        };
        let stored = edited.save_edits(&baseline).unwrap();
        assert_eq!(stored.pipe_capacity_mib, 8);
        let request = uscreen_config::linux::pipe::request_path().unwrap();
        assert!(request.starts_with(std::env::var_os("XDG_RUNTIME_DIR").unwrap()));
        uscreen_config::linux::pipe::publish_current().unwrap();
        assert_eq!(std::fs::read_to_string(request).unwrap(), "8\n");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "t497_default_adapters_publish_only_the_isolated_current_preference",
            "--nocapture",
        ])
        .env("USCREEN_T497_CONFIG_CHILD", "1")
        .env("HOME", root.path())
        .env("XDG_RUNTIME_DIR", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
