//! T495: render shared settings without advertising Linux-only operations.
use super::*;

fn app(store: ConfigStore) -> App {
    let cfg = store.load();
    App {
        camera: camera_settings::Panel::default(),
        _status_worker: None,
        store,
        save: None,
        cfg: cfg.clone(),
        saved_cfg: cfg,
        status: Arc::new(Mutex::new(Status::default())),
        message: String::new(),
        update: Arc::new(Mutex::new(None)),
        tab: Tab::Video,
        action: None,
    }
}

fn text(shape: &egui::Shape, labels: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(value) => labels.push(value.galley.text().to_owned()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| text(shape, labels)),
        _ => {}
    }
}

#[test]
fn t495_preview_hides_linux_setup_and_capacity_controls() {
    let root = tempfile::tempdir().unwrap();
    let mut app = app(ConfigStore::new(root.path().join("config.toml")));
    let ctx = egui::Context::default();
    let mut labels = Vec::new();
    for tab in [Tab::Video, Tab::Display, Tab::General] {
        app.tab = tab;
        let output = ctx.run(egui::RawInput::default(), |ctx| app.show_window(ctx));
        for shape in output.shapes {
            text(&shape.shape, &mut labels);
        }
    }
    assert!(
        labels.iter().any(|label| label.contains("Windows preview")),
        "{labels:?}"
    );
    for forbidden in [
        "Capture pipe buffer",
        "Conversion threads",
        "Setup needed",
        "Daemon stopped",
        "Plug in via USB",
        "Set up display and input (asks for password)",
    ] {
        assert!(
            !labels.iter().any(|label| label.contains(forbidden)),
            "T495: {labels:?}"
        );
    }
}

#[test]
fn t495_unsupported_actions_never_report_success() {
    for result in [
        start_daemon(),
        stop_daemon(),
        restart_daemon(),
        set_autostart(true),
        set_autostart(false),
        run_system_setup(4),
    ] {
        assert!(result.unwrap_err().contains("not implemented on Windows"));
    }
}

#[test]
fn t495_shared_settings_save_in_unicode_directory() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(root.path().join("settings café 東京/config.toml"));
    let mut app = app(store.clone());
    app.cfg.fps = 30;
    app.apply(false);
    let saved = app.save.take().unwrap().finish().unwrap();
    assert_eq!(store.load().fps, 30);
    assert_eq!(saved.message(), "Settings saved");
    assert!(saved.restart.is_none());
    assert!(saved.pipe.is_none());
}

#[test]
fn t495_daemon_discovery_uses_windows_executable_suffix() {
    let root = tempfile::tempdir().unwrap();
    let sibling = root.path().join("uscreen.exe");
    std::fs::write(&sibling, "fixture").unwrap();
    assert_eq!(
        find_uscreen_bin_in(
            Some(root.path().join("uscreen-gui.exe")),
            root.path().join("missing.exe"),
            "".as_ref()
        ),
        Some(sibling)
    );
}
