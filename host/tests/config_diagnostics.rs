//! T559: display and camera settings have distinct identities in diagnostics.
use uscreen_config::{storage::ConfigStore, FileConfig};

fn capture_edit(store: &ConfigStore, edit: impl FnOnce(&mut FileConfig)) -> String {
    let log = tempfile::NamedTempFile::new().unwrap();
    let writer = log.reopen().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.try_clone().unwrap())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        store
            .update(|config| {
                edit(config);
                Ok(())
            })
            .unwrap();
    });
    std::fs::read_to_string(log.path()).unwrap()
}

#[test]
fn t559_unchanged_camera_and_display_settings_produce_no_change_log() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(root.path().join("config.toml"));
    store.update(|_| Ok(())).unwrap();
    let output = capture_edit(&store, |_| {});
    assert!(!output.contains("Config written:"), "T559: {output}");
}

#[test]
fn t559_camera_edit_reports_only_the_qualified_camera_setting() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(root.path().join("config.toml"));
    store.update(|_| Ok(())).unwrap();
    let output = capture_edit(&store, |config| config.camera.options.bitrate = 4500);
    assert!(
        output.contains("camera.bitrate = 4500 (was 3000)"),
        "T559: {output}"
    );
    assert!(!output.contains("height ="), "T559: {output}");
    assert!(!output.contains(", bitrate ="), "T559: {output}");
}
