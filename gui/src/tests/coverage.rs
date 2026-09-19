//! T497: UI callbacks dispatch only isolated fixture commands.
use super::*;

fn fps_frame(
    app: &mut App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    settings_test_frame(app, ctx, events, |app, ui| app.setting_frame_rate(ui))
}
fn position_frame(
    app: &mut App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    settings_test_frame(app, ctx, events, |app, ui| app.setting_position(ui))
}
fn detail_frame(
    app: &mut App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    settings_test_frame(app, ctx, events, |app, ui| app.setting_stream_detail(ui))
}
fn setup_frame(
    app: &mut App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    window_test_frame(app, ctx, events)
}

fn autostart_frame(
    app: &mut App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    settings_test_frame(app, ctx, events, |app, ui| {
        let status = app.status.lock().unwrap().clone();
        app.setting_plug_and_play(ui, &status);
    })
}

fn links_frame(
    app: &mut App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    let output = ctx.run(
        egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.show_header(ui, &Status::default()));
        },
    );
    for command in output.platform_output.commands {
        if let egui::OutputCommand::OpenUrl(url) = command {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("T497 url"), url.url));
        }
    }
    let mut labels = Vec::new();
    for shape in output.shapes {
        collect_text_rects(&shape.shape, &mut labels);
    }
    labels
}

#[test]
fn t497_header_links_only_request_the_expected_fork_pages() {
    let mut app = settings_test_app(Tab::General);
    *app.update.lock().unwrap() = Some("999.0.0".into());
    let ctx = egui::Context::default();
    for (label, expected) in [
        (
            "Report compatibility",
            "https://github.com/geraldo-netto/UScreen/issues/new?",
        ),
        ("Star on GitHub", "https://github.com/geraldo-netto/UScreen"),
        ("open release page", RELEASES_PAGE),
    ] {
        click_settings_text(&mut app, &ctx, label, links_frame);
        let url = ctx
            .data(|data| data.get_temp::<String>(egui::Id::new("T497 url")))
            .unwrap();
        assert!(url.starts_with(expected), "T497 unexpected link: {url}");
    }
}

#[test]
fn t497_display_dropdowns_change_only_the_requested_preference() {
    let mut app = settings_test_app(Tab::Video);
    let ctx = egui::Context::default();
    click_settings_text(&mut app, &ctx, "60 fps", fps_frame);
    click_settings_text(&mut app, &ctx, "30 fps", fps_frame);
    assert_eq!(app.cfg.fps, 30);
    click_settings_text(&mut app, &ctx, "Right of the other screens", position_frame);
    click_settings_text(&mut app, &ctx, "Above the other screens", position_frame);
    assert_eq!(app.cfg.position, "above");
    click_settings_text(&mut app, &ctx, "Full — sharpest", detail_frame);
    click_settings_text(&mut app, &ctx, "Half — lowest latency", detail_frame);
    assert_eq!(app.cfg.stream_scale, 2);
    assert_eq!(app.cfg.fps, 30);
    assert_eq!(app.cfg.width, app.saved_cfg.width + 2);
    for value in [0, 3, u32::MAX] {
        app.cfg.stream_scale = value;
        let labels = detail_frame(&mut app, &ctx, vec![]);
        assert!(labels.iter().any(|(label, _)| label == scale_label(value)));
    }
}

#[test]
fn t497_action_retirement_reports_failure_and_refreshes_status() {
    let mut app = settings_test_app(Tab::Video);
    let (tx, rx) = std::sync::mpsc::channel();
    app.action = Some(rx);
    app.run_action(|| panic!("T497 busy action must not dispatch"));
    drop(tx);
    app.poll_action();
    assert_eq!(app.message, "Action failed");
    assert!(!app.busy());
    app.run_action(|| "Completed fixture action".into());
    wait_for_work(&mut app);
    assert_eq!(app.message, "Completed fixture action");
}

#[test]
fn t497_setup_errors_preserve_the_program_failure() {
    let error = system_setup_result(
        &mut Command::new("/nonexistent/t497-pkexec"),
        Duration::from_secs(1),
    )
    .unwrap_err();
    assert!(error.contains("pkexec failed to run"));
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "echo 'fixture rejection' >&2; exit 42"]);
    assert_eq!(
        system_setup_result(&mut command, Duration::from_secs(1)).unwrap_err(),
        "Setup failed: fixture rejection"
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "echo 'fixture rejection' >&2; exit 42"]);
    assert_eq!(
        execute_daemon_command("start", &mut command, true).unwrap_err(),
        "start failed: fixture rejection"
    );
}

#[test]
fn t497_setup_and_lifecycle_actions_use_private_stub_commands() {
    if std::env::var_os("USCREEN_T497_GUI_ACTIONS").is_none() {
        isolated_actions();
        return;
    }
    let mut app = settings_test_app(Tab::General);
    let ctx = egui::Context::default();
    for count in [-1, 0, 4] {
        *app.status.lock().unwrap() = Status {
            evdi_count: count,
            ..Default::default()
        };
        let labels = setup_frame(&mut app, &ctx, vec![]);
        assert!(labels
            .iter()
            .any(|(label, _)| label.contains("Setup needed")));
        assert!(labels
            .iter()
            .any(|(label, _)| label.contains("ffmpeg, android-tools")));
    }
    click_settings_text(
        &mut app,
        &ctx,
        "Set up display and input (asks for password)",
        setup_frame,
    );
    wait_for_work(&mut app);
    assert_eq!(app.message, "System setup complete");
    for (running, expected) in [(false, "Daemon starting…"), (true, "Daemon stopped")] {
        app.status.lock().unwrap().daemon_running = running;
        click_settings_text(
            &mut app,
            &ctx,
            if running { "Stop" } else { "Start" },
            setup_frame,
        );
        wait_for_work(&mut app);
        assert_eq!(app.message, expected);
    }
    autostart_actions(&mut app, &ctx);
    restart_daemon().unwrap();
    assert_eq!(check_for_update(), Some("999.0.0".into()));
}

fn autostart_actions(app: &mut App, ctx: &egui::Context) {
    for (enabled, message) in [
        (false, "Autostart on — opens the app on a fresh attachment"),
        (true, "Autostart off"),
    ] {
        app.status.lock().unwrap().autostart = enabled;
        click_settings_text(app, ctx, "Start UScreen with the desktop", autostart_frame);
        wait_for_work(app);
        assert_eq!(app.message, message);
    }
}

fn isolated_actions() {
    let sandbox = Sandbox::new();
    let log = sandbox.0.join("actions");
    sandbox.script("pkexec", "echo setup >> \"$USCREEN_T497_GUI_ACTIONS\"");
    sandbox.script(
        "systemctl",
        "printf '%s\\n' \"$*\" >> \"$USCREEN_T497_GUI_ACTIONS\"; case \"$*\" in *LoadState*) echo loaded;; esac",
    );
    sandbox.script(
        "uscreen",
        "printf '%s\\n' \"$*\" >> \"$USCREEN_T497_GUI_ACTIONS\"",
    );
    sandbox.script(
        "curl",
        "echo '{\"tag_name\":\"v999.0.0\",\"draft\":false,\"prerelease\":false}'",
    );
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::coverage::t497_setup_and_lifecycle_actions_use_private_stub_commands",
            "--nocapture",
        ])
        .env("USCREEN_T497_GUI_ACTIONS", &log)
        .env("PATH", &sandbox.0)
        .env("HOME", &sandbox.0)
        .env("XDG_CONFIG_HOME", &sandbox.0)
        .env_remove("APPIMAGE")
        .env_remove("APPDIR")
        .env_remove(uscreen_config::linux::appimage::LAUNCHER)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let actions = std::fs::read_to_string(log).unwrap();
    for expected in [
        "setup",
        "--user start uscreen.service",
        "--user stop uscreen.service",
        "--user restart uscreen.service",
        "--user enable uscreen.service",
        "--user disable uscreen.service",
    ] {
        assert!(
            actions.contains(expected),
            "T497 missing {expected}: {actions}"
        );
    }
}
