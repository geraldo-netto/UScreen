//! T497: diagnostics describe supplied state without changing system settings.
use super::*;

#[test]
fn t497_process_inventory_failure_is_unknown_not_an_empty_success() {
    let mut report = Report::new();
    report_process_query(
        &mut report,
        &FileConfig::default(),
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "fixture process access denied",
        )),
    );
    assert_eq!(report.warnings, 1);
    assert_eq!(report.failures, 0);
    let messages = report.messages.borrow().join("\n");
    assert!(messages.contains("process inspection"));
    assert!(messages.contains("fixture process access denied"));
    assert!(!messages.contains("daemon PID"));
}

#[tokio::test]
async fn t497_virtual_output_diagnostics_tolerate_missing_and_malformed_inventories() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("kscreen");
    let inventory = root.path().join("inventory");
    std::fs::write(&program, "#!/bin/sh\n/bin/cat \"${0%/*}/inventory\"\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let cfg = FileConfig::default();
    let mut report = Report::new();
    check_named_virtual_display(&mut report, &cfg, &[], program.as_os_str()).await;
    assert_eq!(report.failures, 1);
    let connectors = [vdisplay::EvdiConnector {
        name: "fixture-evdi".into(),
        card: 127,
        edid: Vec::new(),
        connected: true,
    }];
    for (text, warnings) in [("invalid", 1), ("{}", 0), (r#"{"outputs":[]}"#, 0)] {
        std::fs::write(&inventory, text).unwrap();
        let mut report = Report::new();
        check_named_virtual_display(&mut report, &cfg, &connectors, program.as_os_str()).await;
        assert_eq!(report.warnings, warnings);
        assert_eq!(report.failures, 0);
        assert!(report.messages.borrow().join("\n").contains("fixture-evdi"));
    }
    std::fs::remove_file(program).unwrap();
    let mut report = Report::new();
    check_named_virtual_display(
        &mut report,
        &cfg,
        &connectors,
        root.path().join("missing").as_os_str(),
    )
    .await;
    assert_eq!(report.failures, 0);
    assert!(report.messages.borrow().join("\n").contains("fixture-evdi"));
}

#[test]
fn t497_module_diagnostics_use_supplied_files_without_loading_drivers() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("sys/devices/evdi")).unwrap();
    std::fs::create_dir_all(root.join("dev")).unwrap();
    let cfg = FileConfig::default();
    let mut report = Report::new();
    check_modules_at(&mut report, &cfg, root);
    assert_eq!(report.failures, 2);
    for count in ["2", "0", "-1", "invalid", "2147483648"] {
        std::fs::write(root.join("sys/devices/evdi/count"), count).unwrap();
        std::fs::write(root.join("dev/uinput"), "").unwrap();
        let mut report = Report::new();
        check_modules_at(&mut report, &cfg, root);
        assert_eq!(report.failures, u32::from(count != "2"));
        assert!(report
            .messages
            .borrow()
            .iter()
            .any(|text| text.contains("writable")));
    }
    std::fs::remove_file(root.join("dev/uinput")).unwrap();
    std::fs::create_dir(root.join("dev/uinput")).unwrap();
    let mut report = Report::new();
    check_modules_at(&mut report, &cfg, root);
    assert_eq!(report.failures, 2);
    assert!(report
        .messages
        .borrow()
        .iter()
        .any(|text| text.contains("not writable")));
}

#[test]
fn t497_disabled_input_and_missing_capacity_are_diagnostics_not_system_changes() {
    let directory = tempfile::tempdir().unwrap();
    let cfg = FileConfig {
        input_touch: false,
        input_pen: false,
        ..Default::default()
    };
    let mut report = Report::new();
    check_modules_at(&mut report, &cfg, directory.path());
    assert_eq!(report.warnings, 1);
    assert_eq!(report.failures, 1);
    for (wanted, cards) in [(2, 0), (8, 2), (128, 128), (u32::MAX, 0), (2, u32::MAX)] {
        let mut report = Report::new();
        report_tablet_capacity(&mut report, wanted, cards);
        assert_eq!(report.warnings, u32::from(cards < wanted));
        assert_eq!(report.failures, 0);
    }
}

#[test]
fn t497_capacity_adapter_reports_requested_multiple_slots() {
    let cfg = FileConfig {
        max_tablets: 2,
        ..Default::default()
    };
    let mut report = Report::new();
    check_tablet_capacity(&mut report, &cfg);
    let messages = report.messages.borrow().join("\n");
    assert!(messages.contains("tablet slots"));
    assert!(messages.contains('2'));
    assert_eq!(report.failures, 0);
}

#[test]
fn t497_colour_profiles_apply_only_to_confirmed_virtual_outputs() {
    let outputs = crate::kscreen::parse(
        br#"{"outputs":[
        {"name":"physical"}, {"name":"DVI-I-1"},
        {"name":"DVI-I-2","iccProfilePath":"/fixture/sRGB.icc"}] }"#,
    )
    .unwrap();
    let mut report = Report::new();
    report_desktop_colour_profiles(&mut report, &["DVI-I-1".into(), "DVI-I-2".into()], &outputs);
    assert_eq!(report.warnings, 1);
    assert_eq!(report.failures, 0);
    let text = report.messages.borrow().join("\n");
    assert!(text.contains("/fixture/sRGB.icc"));
    assert!(!text.contains("physical"));
}

const DIAGNOSTICS_TEST: &str =
    "doctor::coverage_tests::t497_live_adapters_read_only_private_command_fixtures";

fn diagnostics_child() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let tools = directory.path().join("bin");
    std::fs::create_dir(&tools).unwrap();
    for (name, source) in [
        (
            "adb",
            "#!/bin/sh\nfor key do :; done\n/bin/cat \"$HOME/$key\"\n",
        ),
        ("curl", "#!/bin/sh\n/bin/cat \"$HOME/release\"\n"),
        ("kscreen-doctor", "#!/bin/sh\nprintf '{\"outputs\":[]}'\n"),
        ("xinput", "#!/bin/sh\nexit 0\n"),
        ("xrandr", "#!/bin/sh\nexit 0\n"),
    ] {
        let path = tools.join(name);
        std::fs::write(&path, source).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", DIAGNOSTICS_TEST, "--nocapture"])
        .env("BLENT_T497_DIAGNOSTICS", "1")
        .env("PATH", &tools)
        .env("HOME", directory.path())
        .env("XDG_CONFIG_HOME", directory.path())
        .env("XDG_RUNTIME_DIR", directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn colour_queries(root: &Path) {
    for (blue, mode, failures, warnings) in
        [("1", "0", 1, 1), ("0", "2", 0, 0), ("null", "null", 0, 0)]
    {
        std::fs::write(root.join("blue_light_filter"), blue).unwrap();
        std::fs::write(root.join("screen_mode_setting"), mode).unwrap();
        std::fs::write(root.join("refresh_rate_mode"), "0").unwrap();
        let mut report = Report::new();
        check_colour(&mut report, Some("fixture")).await;
        assert_eq!(report.failures, failures);
        assert_eq!(report.warnings, warnings);
        assert!(report
            .messages
            .borrow()
            .iter()
            .any(|text| text.contains("effective display mode")));
    }
    let mut report = Report::new();
    check_colour(&mut report, None).await;
    assert!(report.messages.borrow().is_empty());
}

async fn version_queries(root: &Path) {
    let cfg = FileConfig {
        check_updates: true,
        ..Default::default()
    };
    for (body, warnings, expected) in [
        ("{\"tag_name\":\"v999.0.0\"}", 1, "available"),
        ("{\"tag_name\":\"v0.0.1\"}", 0, "latest"),
        ("invalid", 0, "could not check"),
    ] {
        std::fs::write(root.join("release"), body).unwrap();
        let mut report = Report::new();
        check_version(&mut report, &cfg).await;
        assert_eq!(report.warnings, warnings);
        assert_eq!(report.failures, 0);
        assert!(report
            .messages
            .borrow()
            .iter()
            .any(|text| text.contains(expected)));
    }
}

#[tokio::test]
async fn t497_live_adapters_read_only_private_command_fixtures() {
    if std::env::var_os("BLENT_T497_DIAGNOSTICS").is_none() {
        diagnostics_child();
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    colour_queries(&root).await;
    version_queries(&root).await;
    let mut report = Report::new();
    report_x11_mapping_tools(&mut report);
    assert_eq!(report.failures, 0);
    std::fs::remove_file(root.join("bin/xinput")).unwrap();
    std::fs::remove_file(root.join("bin/xrandr")).unwrap();
    report_x11_mapping_tools(&mut report);
    assert_eq!(report.failures, 2);
    check_kwin_input(&mut report).await;
    assert_eq!(report.failures, 3);
    // Only this re-executed process has its environment changed.
    std::env::set_var("XDG_CONFIG_HOME", "relative");
    std::env::set_var("HOME", "relative");
    check_config(&mut report, &FileConfig::default());
    assert_eq!(report.failures, 4);
}

#[test]
fn t497_input_report_distinguishes_disabled_and_dependent_devices() {
    for flags in 0..16 {
        let cfg = FileConfig {
            input_touch: flags & 1 != 0,
            input_pen: flags & 2 != 0,
            input_pointer: flags & 4 != 0,
            pen_only: flags & 8 != 0,
            ..Default::default()
        };
        let mut report = Report::new();
        report_configured_input(&mut report, &cfg);
        report_input_dependencies(&mut report, &cfg);
        let text = report.messages.borrow().join("\n");
        assert_eq!(
            text.contains("display-only"),
            !cfg.input_touch && !cfg.input_pen
        );
        assert_eq!(report.warnings, u32::from(cfg.pen_only && !cfg.input_pen));
        assert_eq!(report.failures, 0);
    }
}

#[test]
fn t497_display_report_follows_mode_position_and_actual_dimensions() {
    for position in ["left", "right", "above", "below", "unknown"] {
        for enabled in [false, true] {
            let cfg = FileConfig {
                position: position.into(),
                auto_resolution: enabled,
                pen_only: enabled,
                ..Default::default()
            };
            let mut report = Report::new();
            report_configured_display(&mut report, &cfg);
            report_output_mode(&mut report, &cfg, "fixture", 640, 480);
            assert_eq!(report.warnings, u32::from(!enabled));
            assert_eq!(report.failures, 0);
            let text = report.messages.borrow().join("\n");
            assert_eq!(text.contains("follows the tablet"), enabled);
            assert_eq!(text.contains("no display is streamed"), enabled);
        }
    }
}

#[test]
fn t497_connector_and_autostart_reports_preserve_unknown_states() {
    section("T497 isolated diagnostics");
    let mut report = Report::new();
    report_connectors(
        &mut report,
        &[
            vdisplay::EvdiConnector {
                name: "one".into(),
                card: 7,
                edid: Vec::new(),
                connected: true,
            },
            vdisplay::EvdiConnector {
                name: "two".into(),
                card: 8,
                edid: Vec::new(),
                connected: false,
            },
        ],
    );
    assert_eq!(report.warnings, 1);
    assert!(report
        .messages
        .borrow()
        .join("\n")
        .contains("two (disconnected"));
    for (load, enabled, warnings) in [
        ("not-found", Some("enabled"), 1),
        ("loaded", Some("enabled\n"), 0),
        ("loaded", Some("disabled"), 1),
        ("loaded", None, 0),
    ] {
        let mut report = Report::new();
        report_autostart(&mut report, load, enabled.map(str::to_owned));
        assert_eq!(report.warnings, warnings);
        assert_eq!(report.failures, 0);
    }
}

#[tokio::test]
async fn t497_helper_probe_accepts_usage_and_reports_process_failures() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let helper = directory.path().join("fake-helper");
    for (body, failures) in [
        ("exit 0", 0),
        ("echo Usage: >&2; exit 1", 0),
        ("echo loader-error >&2; exit 2", 1),
    ] {
        std::fs::write(&helper, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut report = Report::new();
        check_helper_execution(&mut report, &helper).await;
        assert_eq!(report.failures, failures);
    }
    std::fs::remove_file(&helper).unwrap();
    let mut report = Report::new();
    check_helper_execution(&mut report, &helper).await;
    assert_eq!(report.failures, 1);
}
