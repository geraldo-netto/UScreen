#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pipe_settings;
mod settings;
mod status_poll;
mod status_worker;

use eframe::egui;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uscreen_config::commands::SyncCommandExt;
use uscreen_config::commands::{daemon_command_timeout, spawn_reaped};
use uscreen_config::linux::daemon;
use uscreen_config::model::{
    FileConfig, MAX_BITRATE_KBPS, MAX_DIMENSION, MAX_QUALITY, MIN_BITRATE_KBPS, MIN_QUALITY,
};
use uscreen_config::storage::{config_path, ConfigStore};

/// Autostart can be a user service or an XDG desktop entry.
fn autostart_enabled() -> bool {
    uscreen_config::linux::autostart::enabled()
}

fn set_autostart(on: bool) -> Result<(), String> {
    set_autostart_with(on, || !daemon::discover(Some(&pid_path())).is_empty())
}

fn set_autostart_with(on: bool, running: impl Fn() -> bool) -> Result<(), String> {
    let bin = if on {
        find_uscreen_bin().ok_or("uscreen binary not found")?
    } else {
        PathBuf::new()
    };
    uscreen_config::linux::autostart::set_enabled(on, &bin).map_err(|e| e.to_string())?;
    let result = if on {
        if !running() {
            start_daemon_with(service_managed_with(&running))
        } else {
            Ok(())
        }
    } else {
        run_daemon_command("stop", service_managed_with(&running))
    };
    result.map_err(|e| format!("Autostart preference saved; daemon action failed: {e}"))
}

#[derive(Default, Clone, PartialEq, Eq)]
struct Status {
    daemon_running: bool,
    daemon_pid: u32,
    tablet_connected: bool,
    tablet_model: String,
    /// -1 = evdi module not loaded, otherwise the device count
    evdi_count: i32,
    ffmpeg_ok: bool,
    adb_ok: bool,
    autostart: bool,
    uinput_ok: bool,
    pipe_capacities: Vec<(u32, Option<u32>)>,
    pipe_ceiling: Option<u32>,
}

fn needs_system_setup(status: &Status, config: &FileConfig) -> bool {
    status.evdi_count < config.max_tablets.clamp(1, 4) as i32
        || ((config.input_touch || config.input_pen) && !status.uinput_ok)
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn pid_path() -> PathBuf {
    PathBuf::from(format!("{}/.local/share/uscreen/uscreen.pid", home()))
}

fn find_uscreen_bin() -> Option<PathBuf> {
    find_uscreen_bin_in(
        std::env::current_exe().ok(),
        PathBuf::from(format!("{}/.local/bin/uscreen", home())),
        &std::env::var_os("PATH").unwrap_or_default(),
    )
}

fn find_uscreen_bin_in(
    exe: Option<PathBuf>,
    installed: PathBuf,
    path: &std::ffi::OsStr,
) -> Option<PathBuf> {
    use uscreen_config::linux::programs::{find_in, is_executable};
    if let Some(sibling) = exe.and_then(|exe| exe.parent().map(|dir| dir.join("uscreen"))) {
        if is_executable(&sibling) {
            return Some(sibling);
        }
    }
    find_in("uscreen", path).or_else(|| is_executable(&installed).then_some(installed))
}

use uscreen_config::linux::programs::command_exists;

#[cfg(test)]
fn poll_status() -> Status {
    status_poll::StatusPoller::default().poll(true)
}

#[cfg(test)]
fn apply_tablet_status(s: &mut Status, text: &str, sessions_path: Option<&std::path::Path>) {
    let sessions = sessions_path
        .and_then(uscreen_config::runtime::load_sessions)
        .unwrap_or_default();
    apply_tablet_sessions(s, text, sessions);
}

fn apply_tablet_sessions(
    s: &mut Status,
    text: &str,
    mut sessions: Vec<uscreen_config::runtime::TabletSession>,
) {
    sessions.sort_by_key(|session| session.instance);
    let models: Vec<_> = sessions
        .iter()
        .filter_map(|session| {
            let line = text.lines().find(|line| {
                let mut fields = line.split_whitespace();
                fields.next() == Some(session.serial.as_str()) && fields.next() == Some("device")
            })?;
            Some(
                line.split_whitespace()
                    .find_map(|field| field.strip_prefix("model:"))
                    .unwrap_or(&session.serial)
                    .replace('_', " "),
            )
        })
        .collect();
    s.tablet_connected = !models.is_empty();
    s.tablet_model = models.join(", ");
}

/// One-time privileged setup via the desktop's graphical password prompt:
/// pre-create an EVDI device now and at every boot.
fn system_setup_script(root: &std::path::Path, max_tablets: u32) -> String {
    let count = max_tablets.clamp(1, 4);
    let script = format!(
        r#"set -e
mkdir -p /etc/modprobe.d /etc/modules-load.d
echo 'options evdi initial_device_count={count}' > /etc/modprobe.d/uscreen-evdi.conf
printf 'evdi\nuinput\n' > /etc/modules-load.d/uscreen.conf
modprobe evdi || true
modprobe uinput || true
existing=$(cat /sys/devices/evdi/count 2>/dev/null || echo 0)
if [ "$existing" -lt {count} ]; then
    echo "$(({count} - existing))" > /sys/devices/evdi/add
fi"#
    );
    let script = format!("{}\nmkdir -p /etc/udev/rules.d\ncat > /etc/udev/rules.d/60-uscreen-uinput.rules <<'USCREEN_RULE'\n{}USCREEN_RULE\nudevadm control --reload\nudevadm trigger --name-match=uinput\n", script,
        include_str!("../../packaging/60-uscreen-uinput.rules"));
    script
        .replace("/etc/", &format!("{}/etc/", root.display()))
        .replace("/sys/", &format!("{}/sys/", root.display()))
}

fn run_system_setup(max_tablets: u32) -> Result<(), String> {
    let script = system_setup_script(std::path::Path::new("/"), max_tablets);
    system_setup_result(
        Command::new("pkexec").args(["sh", "-c", &script]),
        Duration::from_secs(120),
    )
}

fn system_setup_result(command: &mut Command, timeout: Duration) -> Result<(), String> {
    let out = command
        .output_timeout(timeout)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                "Setup response timed out. Setup may still be running with elevated permissions. Wait and check its status before trying again.".to_owned()
            } else {
                format!("pkexec failed to run: {e}")
            }
        })?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Setup failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn daemon_command(bin: &std::path::Path, action: &str, managed: bool) -> Command {
    if managed {
        let mut command = Command::new("systemctl");
        command.args(["--user", action, "uscreen.service"]);
        command
    } else {
        let mut command = Command::new(bin);
        command.arg(action);
        command
    }
}

fn service_managed() -> bool {
    service_managed_with(|| !daemon::discover(Some(&pid_path())).is_empty())
}

fn service_managed_with(running: impl FnOnce() -> bool) -> bool {
    if Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "uscreen.service"])
        .output_bounded()
        .is_ok_and(|output| output.status.success())
    {
        return true;
    }
    if running() {
        return false;
    }
    uscreen_config::linux::autostart::systemd_available()
}

fn run_daemon_command(action: &str, managed: bool) -> Result<(), String> {
    let bin = if managed {
        PathBuf::new()
    } else {
        find_uscreen_bin().ok_or("uscreen binary not found")?
    };
    execute_daemon_command(action, &mut daemon_command(&bin, action, managed), managed)
}

fn execute_daemon_command(
    action: &str,
    command: &mut Command,
    managed: bool,
) -> Result<(), String> {
    let output = command
        .output_timeout(daemon_command_timeout(managed))
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} failed: {}",
            action,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn restart_daemon() -> Result<(), String> {
    restart_with(service_managed(), run_daemon_command, start_direct_daemon)
}

fn restart_with(
    managed: bool,
    mut run: impl FnMut(&str, bool) -> Result<(), String>,
    start: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if managed {
        return run("restart", true);
    }
    run("stop", false)?;
    start()
}

fn start_daemon() -> Result<(), String> {
    start_daemon_with(service_managed())
}

fn start_daemon_with(managed: bool) -> Result<(), String> {
    if managed {
        return run_daemon_command("start", true);
    }
    start_direct_daemon()
}

fn start_direct_daemon() -> Result<(), String> {
    let bin = find_uscreen_bin().ok_or("uscreen binary not found — run `make install`")?;
    let log_dir = PathBuf::from(format!("{}/.local/share/uscreen", home()));
    let _ = std::fs::create_dir_all(&log_dir);
    let log = std::fs::File::create(log_dir.join("daemon.log")).map_err(|e| e.to_string())?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;
    let mut child = daemon_command(&bin, "start", false)
        .stdout(log)
        .stderr(log_err)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start daemon: {}", e))?;
    // The GUI launches the foreground daemon as a child. The
    // std::process::Child handle must still be waited on or the kernel
    // leaves a zombie behind once it exits. Reap it on a background thread
    // instead of blocking the GUI.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn stop_daemon() -> Result<(), String> {
    run_daemon_command("stop", service_managed())
}

fn dispatch_action(
    action: impl FnOnce() -> String + Send + 'static,
) -> std::sync::mpsc::Receiver<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(action());
    });
    receiver
}

struct App {
    _status_worker: Option<status_worker::StatusWorker>,
    store: ConfigStore,
    save: Option<settings::PendingSave>,
    cfg: FileConfig,
    saved_cfg: FileConfig,
    status: Arc<Mutex<Status>>,
    message: String,
    /// Newer release, if the check made when the window opened found one.
    update: Arc<Mutex<Option<String>>>,
    tab: Tab,
    action: Option<std::sync::mpsc::Receiver<String>>,
}

/// The settings are more than fit in one column, so they are grouped.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Tab {
    /// Encoder, quality, bitrate, frame rate, colour depth, resolution, scale.
    Video,
    /// Where the screen goes, which mode, how many tablets, input devices.
    Display,
    /// Security, updates, plug & play.
    General,
}

use uscreen_config::release::{API as RELEASES_API, PAGE as RELEASES_PAGE};

fn os_release_name() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_string())
            })
        })
        .unwrap_or_default()
        + " / "
        + &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()
        + " ("
        + &std::env::var("XDG_SESSION_TYPE").unwrap_or_default()
        + ")"
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn compatibility_url(distro: &str, encoder: &str, tablet: &str, version: &str) -> String {
    // YAML issue forms prefill by field ID, as declared in compatibility.yml.
    let query = [
        ("template", "compatibility.yml"),
        ("title", "Compatibility: "),
        ("distro", distro),
        ("gpu", encoder),
        ("tablet", tablet),
        ("version", version),
    ]
    .into_iter()
    .map(|(key, value)| format!("{key}={}", urlencode(value)))
    .collect::<Vec<_>>()
    .join("&");
    format!("https://github.com/geraldo-netto/UScreen/issues/new?{query}")
}

use uscreen_config::release::newer_from_json as release_from_response;
#[cfg(test)]
use uscreen_config::version::is_newer as is_newer_version;

/// One request when the window opens. Reports; never installs.
fn check_for_update() -> Option<String> {
    let out = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "4",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            concat!("User-Agent: uscreen-gui/", env!("CARGO_PKG_VERSION")),
            RELEASES_API,
        ])
        .output_bounded()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    release_from_response(
        &String::from_utf8_lossy(&out.stdout),
        env!("CARGO_PKG_VERSION"),
    )
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = FileConfig::load();
        let status = Arc::new(Mutex::new(Status::default()));

        let update = Arc::new(Mutex::new(None));
        if cfg.check_updates {
            let slot = update.clone();
            let repaint = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                if let Some(v) = check_for_update() {
                    if let Ok(mut g) = slot.lock() {
                        *g = Some(v);
                        repaint.request_repaint();
                    }
                }
            });
        }

        // Background poller: daemon + adb state every 2 seconds
        let status_bg = status.clone();
        let repaint = cc.egui_ctx.clone();
        let mut sampler = status_poll::StatusPoller::default();
        let worker = status_worker::StatusWorker::start(
            move |force| {
                let s = sampler.poll(force);
                if let Ok(mut guard) = status_bg.lock() {
                    if *guard != s {
                        *guard = s;
                        repaint.request_repaint();
                    }
                }
            },
            Duration::from_secs(2),
        );

        Self {
            _status_worker: Some(worker),
            store: ConfigStore::default(),
            save: None,
            saved_cfg: cfg.clone(),
            cfg,
            status,
            message: String::new(),
            update,
            tab: Tab::Video,
            action: None,
        }
    }

    fn run_action(&mut self, action: impl FnOnce() -> String + Send + 'static) {
        if !self.busy() {
            self.message = "Working…".into();
            self.action = Some(dispatch_action(action));
        }
    }

    fn apply(&mut self, restart: bool) {
        if self.busy() {
            return;
        }
        let restart = (restart && self.cfg.requires_restart_from(&self.saved_cfg))
            .then(|| Box::new(restart_daemon) as settings::Restart);
        self.save = Some(settings::PendingSave::start(
            self.store.clone(),
            self.cfg.clone(),
            self.saved_cfg.clone(),
            restart,
        ));
        self.message = "Saving…".into();
    }

    fn busy(&self) -> bool {
        self.action.is_some() || self.save.is_some()
    }

    fn poll_save(&mut self) {
        if !self.save.as_ref().is_some_and(|save| save.is_finished()) {
            return;
        }
        let save = self.save.take().unwrap();
        let submitted = save.submitted.clone();
        match save.finish() {
            Ok(saved) => {
                self.message = saved.message();
                // Only edits made after submission override the committed
                // snapshot, including concurrent disk changes merged by it.
                if let Ok(edited) = self.cfg.merge_edits(&submitted, saved.config.clone()) {
                    self.cfg = edited;
                }
                self.saved_cfg = saved.config;
            }
            Err(error) => self.message = error,
        }
    }
}

fn bitrate_slider(ui: &mut egui::Ui, bitrate: &mut u32) -> egui::Response {
    let mut mbps = *bitrate as f32 / 1000.0;
    let response = ui.add(
        egui::Slider::new(
            &mut mbps,
            MIN_BITRATE_KBPS as f32 / 1000.0..=MAX_BITRATE_KBPS as f32 / 1000.0,
        )
        .suffix(" Mbps"),
    );
    if response.changed() {
        *bitrate = (mbps * 1000.0) as u32;
    }
    response
}

fn scale_label(n: u32) -> &'static str {
    match n {
        3 => "Third — very soft",
        _ => "Quarter — very soft",
    }
}

fn status_dot(ui: &mut egui::Ui, on: bool, label: &str, detail: &str) {
    ui.horizontal(|ui| {
        let color = if on {
            egui::Color32::from_rgb(76, 175, 80)
        } else {
            egui::Color32::from_rgb(120, 120, 130)
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 5.0, color);
        ui.label(egui::RichText::new(label).strong());
        if !detail.is_empty() {
            ui.label(egui::RichText::new(detail).weak());
        }
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show_window(ctx);
    }
}

impl App {
    fn show_window(&mut self, ctx: &egui::Context) {
        self.poll_action();
        let status = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        self.show_footer(ctx, &status);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    self.show_header(ui, &status);
                    self.show_setup(ui, &status);
                    self.show_status(ui, &status);
                    self.show_daemon_control(ui, status.daemon_running);
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(8.0);
                    self.show_settings(ui, &status);
                });
        });
        // Include work started by a button during this frame.
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn poll_action(&mut self) {
        let was_busy = self.busy();
        self.poll_save();
        if let Some(receiver) = &self.action {
            match receiver.try_recv() {
                Ok(message) => {
                    self.message = message;
                    self.action = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.message = "Action failed".into();
                    self.action = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if was_busy && !self.busy() {
            if let Some(worker) = &self._status_worker {
                worker.refresh();
            }
        }
    }

    fn show_footer(&mut self, ctx: &egui::Context, status: &Status) {
        // Keep one shared action row below every tab, visible while settings scroll.
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.add_space(8.0);
            let dirty = self.cfg != self.saved_cfg;
            ui.horizontal(|ui| {
                let label =
                    settings::apply_label(status.daemon_running, &self.cfg, &self.saved_cfg);
                if ui
                    .add_enabled(dirty && !self.busy(), egui::Button::new(label))
                    .clicked()
                {
                    self.apply(status.daemon_running);
                }
                if dirty
                    && ui
                        .add_enabled(!self.busy(), egui::Button::new("Discard"))
                        .clicked()
                {
                    self.cfg = self.saved_cfg.clone();
                }
            });
            if !self.message.is_empty() {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(&self.message).weak());
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("config: {}", config_path().display()))
                    .weak()
                    .size(10.0),
            );
            ui.add_space(2.0);
        });
    }

    fn show_header(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.add_space(6.0);
        ui.heading(egui::RichText::new("UScreen").size(26.0));
        ui.label(egui::RichText::new("USB second display for your tablet").weak());
        ui.horizontal(|ui| {
            if ui.small_button("Report compatibility").on_hover_text(
                "Opens a GitHub issue pre-filled with your setup. Nothing is sent until you submit it.").clicked()
            {
                let url = compatibility_url(&os_release_name(), &self.cfg.encoder,
                    &status.tablet_model, env!("CARGO_PKG_VERSION"));
                let _ = spawn_reaped(Command::new("xdg-open").arg(url));
            }
            if ui.small_button("Star on GitHub").clicked() {
                let _ = spawn_reaped(Command::new("xdg-open").arg("https://github.com/geraldo-netto/UScreen"));
            }
        });
        if let Some(v) = self.update.lock().ok().and_then(|g| g.clone()) {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Update available: {}", v)).strong());
                if ui.link("open release page").clicked() {
                    let _ = spawn_reaped(Command::new("xdg-open").arg(RELEASES_PAGE));
                }
            });
        }
        ui.add_space(12.0);
    }

    fn show_missing_packages(&mut self, ui: &mut egui::Ui, status: &Status) {
        let mut pkgs = vec![];
        if !status.ffmpeg_ok {
            pkgs.push("ffmpeg");
        }
        if !status.adb_ok {
            pkgs.push("android-tools (adb)");
        }
        ui.label(format!(
            "Install with your package manager: {}",
            pkgs.join(", ")
        ));
    }

    fn show_system_setup(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.label(if status.evdi_count < 0 {
            "The EVDI kernel module is not loaded (install evdi/evdi-dkms)."
        } else if status.evdi_count < self.cfg.max_tablets as i32 {
            "More virtual display devices are needed for the configured tablet count."
        } else {
            "Touch and pen input need permission to access /dev/uinput."
        });
        if ui
            .button("Set up display and input (asks for password)")
            .clicked()
        {
            let max_tablets = self.cfg.max_tablets;
            self.run_action(move || {
                run_system_setup(max_tablets)
                    .map(|_| "System setup complete".into())
                    .unwrap_or_else(|e| e)
            });
        }
    }

    fn show_setup(&mut self, ui: &mut egui::Ui, status: &Status) {
        let needs_setup = needs_system_setup(status, &self.cfg);
        let missing_pkgs = !status.ffmpeg_ok || !status.adb_ok;
        if needs_setup || missing_pkgs {
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgb(50, 38, 22))
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        egui::RichText::new("Setup needed")
                            .strong()
                            .color(egui::Color32::from_rgb(255, 180, 80)),
                    );
                    if missing_pkgs {
                        self.show_missing_packages(ui, status);
                    }
                    if needs_setup {
                        self.show_system_setup(ui, status);
                    }
                });
            ui.add_space(10.0);
        }
    }

    fn show_status(&mut self, ui: &mut egui::Ui, status: &Status) {
        // ----- Status -----
        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                status_dot(
                    ui,
                    status.daemon_running,
                    if status.daemon_running {
                        "Daemon running"
                    } else {
                        "Daemon stopped"
                    },
                    &if status.daemon_running {
                        format!("PID {}", status.daemon_pid)
                    } else {
                        String::new()
                    },
                );
                ui.add_space(4.0);
                status_dot(
                    ui,
                    status.tablet_connected,
                    if status.tablet_connected {
                        "Tablet connected"
                    } else {
                        "No tablet detected"
                    },
                    &status.tablet_model,
                );
                if !status.tablet_connected {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "Plug in via USB and enable USB debugging on the tablet",
                        )
                        .weak()
                        .size(11.0),
                    );
                }
            });

        ui.add_space(10.0);
    }

    fn show_daemon_control(&mut self, ui: &mut egui::Ui, running: bool) {
        ui.horizontal(|ui| {
            let big = egui::vec2(ui.available_width(), 34.0);
            let label = if running { "Stop" } else { "Start" };
            if ui
                .add_sized(
                    big,
                    egui::Button::new(egui::RichText::new(label).size(16.0)),
                )
                .clicked()
            {
                self.request_daemon_state(running);
            }
        });
    }

    fn request_daemon_state(&mut self, running: bool) {
        self.run_action(move || {
            if running {
                stop_daemon()
                    .map(|_| "Daemon stopped".into())
                    .unwrap_or_else(|e| e)
            } else {
                start_daemon()
                    .map(|_| "Daemon starting…".into())
                    .unwrap_or_else(|e| e)
            }
        });
    }

    fn setting_encoder(&mut self, ui: &mut egui::Ui) {
        ui.label("Encoder");
        let selected = if self.cfg.encoder == "auto" {
            "Automatic (measure compatible encoders)"
        } else {
            uscreen_config::encoding::find(&self.cfg.encoder)
                .map(|encoder| encoder.label)
                .unwrap_or(&self.cfg.encoder)
        };
        egui::ComboBox::from_id_salt("encoder")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.cfg.encoder,
                    "auto".to_string(),
                    "Automatic (measure compatible encoders)",
                );
                for encoder in uscreen_config::encoding::ENCODERS {
                    ui.selectable_value(
                        &mut self.cfg.encoder,
                        encoder.name.to_string(),
                        encoder.label,
                    );
                }
            });
        ui.end_row();
    }

    fn setting_quality(&mut self, ui: &mut egui::Ui) {
        ui.label("Quality");
        ui.vertical(|ui| {
            // Shown the intuitive way round — dragging right means
            // sharper — while the stored value is the encoder's
            // quantiser, where lower is better.
            let mut sharpness =
                (MAX_QUALITY - self.cfg.quality.clamp(MIN_QUALITY, MAX_QUALITY)) as f32;
            let span = (MAX_QUALITY - MIN_QUALITY) as f32;
            if ui
                .add(egui::Slider::new(&mut sharpness, 0.0..=span).show_value(false))
                .changed()
            {
                self.cfg.quality = MAX_QUALITY - sharpness.round() as u32;
            }
            ui.label(
                egui::RichText::new("This, not the bitrate, sets how sharp text looks")
                    .weak()
                    .size(11.0),
            );
        });
        ui.end_row();
    }

    fn setting_bitrate(&mut self, ui: &mut egui::Ui) {
        ui.label("Bitrate ceiling");
        ui.vertical(|ui| {
            bitrate_slider(ui, &mut self.cfg.bitrate);
            ui.label(
                egui::RichText::new("Only a cap for bursts — a desktop streams well below it")
                    .weak()
                    .size(11.0),
            );
        });
        ui.end_row();
    }

    fn setting_frame_rate(&mut self, ui: &mut egui::Ui) {
        ui.label("Frame rate");
        // 120 is not offered: EDID 1.4 stores the pixel clock in 16
        // bits and 2960x1848@120 overflows it, so the virtual mode
        // is capped at 90 Hz.
        egui::ComboBox::from_id_salt("fps")
            .selected_text(format!("{} fps", self.cfg.fps))
            .show_ui(ui, |ui| {
                for f in [30u32, 60, 90] {
                    ui.selectable_value(&mut self.cfg.fps, f, format!("{} fps", f));
                }
            });
        ui.end_row();
    }

    fn setting_security(&mut self, ui: &mut egui::Ui) {
        ui.label("Security");
        ui.vertical(|ui| {
            ui.checkbox(&mut self.cfg.require_token, "Require the session token");
            ui.label(
                egui::RichText::new(
                    "Off only for an app older than 1.1.0. Without it any local process \
                 can read the screen and inject input.",
                )
                .small()
                .weak(),
            );
        });
        ui.end_row();
    }

    fn setting_updates(&mut self, ui: &mut egui::Ui) {
        ui.label("Updates");
        ui.checkbox(
            &mut self.cfg.check_updates,
            "Check for a newer release on start",
        );
        ui.end_row();
    }

    fn setting_tablets(&mut self, ui: &mut egui::Ui) {
        ui.label("Tablets");
        ui.vertical(|ui| {
            ui.add(egui::Slider::new(&mut self.cfg.max_tablets, 1..=4).text("at once"));
            ui.label(
                egui::RichText::new(
                    "Each tablet becomes its own screen. Needs that many EVDI devices \
                 (see uscreen doctor); the installer prepares two.",
                )
                .small()
                .weak(),
            );
        });
        ui.end_row();
    }

    fn setting_position(&mut self, ui: &mut egui::Ui) {
        ui.label("Position");
        egui::ComboBox::from_id_salt("position")
            .selected_text(match self.cfg.position.as_str() {
                "left" => "Left of the other screens",
                "above" => "Above the other screens",
                "below" => "Below the other screens",
                _ => "Right of the other screens",
            })
            .show_ui(ui, |ui| {
                for (v, label) in [
                    ("right", "Right of the other screens"),
                    ("left", "Left of the other screens"),
                    ("above", "Above the other screens"),
                    ("below", "Below the other screens"),
                ] {
                    ui.selectable_value(&mut self.cfg.position, v.to_string(), label);
                }
            });
        ui.end_row();
    }

    fn setting_colour_depth(&mut self, ui: &mut egui::Ui) {
        ui.label("Colour depth");
        ui.vertical(|ui| {
            let hevc = uscreen_config::encoding::find(&self.cfg.encoder)
                .is_some_and(|encoder| encoder.hevc);
            ui.add_enabled(
                hevc,
                egui::Checkbox::new(&mut self.cfg.ten_bit, "10-bit (HEVC Main10)"),
            );
            ui.label(
                egui::RichText::new(if hevc {
                    "Smooths banding on gradients. The desktop itself is 8-bit, \
                     so this adds precision, not colour."
                } else {
                    "Needs the HEVC encoder — H.264 here is 8-bit only."
                })
                .small()
                .weak(),
            );
        });
        ui.end_row();
    }

    fn setting_resolution(&mut self, ui: &mut egui::Ui) {
        ui.label("Resolution");
        ui.vertical(|ui| {
            ui.checkbox(&mut self.cfg.auto_resolution, "Auto (match the tablet)");
            ui.horizontal(|ui| {
                ui.add_enabled(
                    !self.cfg.auto_resolution,
                    egui::DragValue::new(&mut self.cfg.width)
                        .range(640..=MAX_DIMENSION)
                        .speed(8),
                );
                ui.label("×");
                ui.add_enabled(
                    !self.cfg.auto_resolution,
                    egui::DragValue::new(&mut self.cfg.height)
                        .range(480..=MAX_DIMENSION)
                        .speed(8),
                );
            });
            if self.cfg.auto_resolution {
                ui.label(
                    egui::RichText::new(format!(
                        "currently {} × {}",
                        self.cfg.width, self.cfg.height
                    ))
                    .weak()
                    .size(11.0),
                );
            }
        });
        ui.end_row();
    }

    fn setting_mode(&mut self, ui: &mut egui::Ui) {
        ui.label("Mode");
        ui.vertical(|ui| {
            ui.checkbox(
                &mut self.cfg.pen_only,
                "Graphics tablet instead of a second screen",
            );
            ui.label(
                egui::RichText::new(
                    "Nothing is streamed: the pen drives this machine's own \
                     screen, so there is no display latency at all. Pressure, \
                     tilt and the eraser still work.",
                )
                .weak()
                .size(11.0),
            );
        });
        ui.end_row();
    }

    fn setting_input_devices(&mut self, ui: &mut egui::Ui) {
        ui.label("Input devices");
        ui.vertical(|ui| {
            ui.checkbox(
                &mut self.cfg.input_touch,
                "Touchscreen (taps on the tablet)",
            );
            ui.checkbox(
                &mut self.cfg.input_pen,
                "Pen tablet (stylus, pressure, tilt)",
            );
            // Preserve the dormant preference. The daemon creates a pointer
            // only when both pen and pointer are enabled.
            ui.add_enabled(
                self.cfg.input_pen,
                egui::Checkbox::new(
                    &mut self.cfg.input_pointer,
                    "Pointer that stays where the pen lifted",
                ),
            );
            ui.label(
                egui::RichText::new(
                    "Each one is a virtual input device the desktop sees while a \
                     tablet is attached. Turn off what you do not use: on \
                     Cinnamon/GNOME under X11 a touchscreen device can make the \
                     mouse cursor hide. Restart the daemon to apply.",
                )
                .weak()
                .size(11.0),
            );
        });
        ui.end_row();
    }

    fn setting_stream_detail(&mut self, ui: &mut egui::Ui) {
        ui.label("Stream detail");
        ui.vertical(|ui| {
            egui::ComboBox::from_id_salt("stream_scale")
                .selected_text(match self.cfg.stream_scale {
                    1 => "Full — sharpest",
                    2 => "Half — lowest latency",
                    n => scale_label(n),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cfg.stream_scale, 1, "Full — sharpest");
                    ui.selectable_value(&mut self.cfg.stream_scale, 2, "Half — lowest latency");
                });
            ui.label(
                egui::RichText::new(
                    "The desktop keeps its full resolution either way. Half sends \
                     a quarter of the pixels, which the tablet decodes sooner — \
                     good for games, softer for text.",
                )
                .weak()
                .size(11.0),
            );
        });
        ui.end_row();
    }

    fn setting_plug_and_play(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.label("Plug & play");
        ui.vertical(|ui| {
            ui.checkbox(
                &mut self.cfg.auto_launch_app,
                "Open the app on the tablet automatically",
            );
            let mut auto = status.autostart;
            if ui
                .checkbox(&mut auto, "Start UScreen with the desktop")
                .changed()
            {
                self.run_action(move || {
                    set_autostart(auto)
                        .map(|_| {
                            if auto {
                                "Autostart on — plugging the cable in is now enough".into()
                            } else {
                                "Autostart off".into()
                            }
                        })
                        .unwrap_or_else(|e| e)
                });
            }
        });
        ui.end_row();
    }

    fn show_video_settings(&mut self, ui: &mut egui::Ui) {
        self.setting_encoder(ui);
        self.setting_quality(ui);
        self.setting_bitrate(ui);
        self.setting_frame_rate(ui);
        self.setting_colour_depth(ui);
        self.setting_resolution(ui);
        self.setting_stream_detail(ui);
        let status = self.status.lock().unwrap().clone();
        pipe_settings::show(ui, &mut self.cfg.pipe_capacity_mib, &status);
    }

    fn show_display_settings(&mut self, ui: &mut egui::Ui) {
        self.setting_tablets(ui);
        self.setting_position(ui);
        self.setting_mode(ui);
        self.setting_input_devices(ui);
    }

    fn show_general_settings(&mut self, ui: &mut egui::Ui, status: &Status) {
        self.setting_security(ui);
        self.setting_updates(ui);
        self.setting_plug_and_play(ui, status);
    }

    fn show_settings(&mut self, ui: &mut egui::Ui, status: &Status) {
        ui.label(egui::RichText::new("Settings").strong().size(15.0));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (tab, name) in [
                (Tab::Video, "Video"),
                (Tab::Display, "Display & input"),
                (Tab::General, "General"),
            ] {
                ui.selectable_value(&mut self.tab, tab, name);
            }
        });
        ui.add_space(8.0);
        // Each tab retains its own grid ID and remembered column widths.
        egui::Grid::new(("settings", self.tab))
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| match self.tab {
                Tab::Video => self.show_video_settings(ui),
                Tab::Display => self.show_display_settings(ui),
                Tab::General => self.show_general_settings(ui, status),
            });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Tall enough for the longest settings tab; anything shorter
            // scrolls rather than hiding rows off the bottom.
            .with_inner_size([440.0, 760.0])
            .with_min_inner_size([380.0, 560.0])
            .with_app_id("uscreen")
            // Window icon from the same picture as the launcher and tray, so
            // the task bar shows it even where the theme icon is not installed.
            .with_icon(egui::IconData {
                rgba: include_bytes!("../../packaging/icons/uscreen-64.rgba").to_vec(),
                width: 64,
                height: 64,
            }),
        // Open in the middle of the screen rather than wherever the window
        // manager drops it (Wayland compositors cannot honour this; X11 can).
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "UScreen",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn t374_release_json_contract() {
        for line in include_str!("../../testdata/release-responses.tsv").lines() {
            let parts: Vec<_> = line.split('\t').collect();
            let expected = (parts[2] != "-").then_some(parts[2]);
            assert_eq!(
                release_from_response(parts[0], "1.2.3").as_deref(),
                expected,
                "T374: {line}"
            );
        }
    }

    mod lookup_fixture {
        include!("../../testdata/executable_lookup.rs");
    }

    #[test]
    fn t427_daemon_lookup_skips_unusable_candidates() {
        let root = tempfile::tempdir().unwrap();
        let sibling = root.path().join("sibling");
        let earlier = root.path().join("earlier");
        let later = root.path().join("later");
        for directory in [&sibling, &earlier, &later] {
            std::fs::create_dir(directory).unwrap();
        }
        let valid = later.join("uscreen");
        std::fs::write(&valid, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&valid, std::fs::Permissions::from_mode(0o700)).unwrap();
        let installed = root.path().join("installed");
        std::os::unix::fs::symlink(&valid, &installed).unwrap();
        let path = std::env::join_paths([&earlier, &later]).unwrap();
        let exe = Some(sibling.join("uscreen-gui"));
        std::fs::create_dir(sibling.join("uscreen")).unwrap();
        std::fs::write(earlier.join("uscreen"), "not executable").unwrap();
        assert_eq!(
            find_uscreen_bin_in(exe.clone(), installed.clone(), &path),
            Some(valid.clone())
        );
        std::fs::remove_dir(sibling.join("uscreen")).unwrap();
        std::os::unix::fs::symlink(root.path().join("missing"), sibling.join("uscreen")).unwrap();
        assert_eq!(
            find_uscreen_bin_in(exe.clone(), installed.clone(), &path),
            Some(valid.clone())
        );
        std::fs::remove_file(sibling.join("uscreen")).unwrap();
        std::os::unix::fs::symlink(&valid, sibling.join("uscreen")).unwrap();
        assert_eq!(
            find_uscreen_bin_in(exe, installed.clone(), &path),
            Some(sibling.join("uscreen"))
        );
        let path = std::env::join_paths([&earlier]).unwrap();
        assert_eq!(
            find_uscreen_bin_in(None, installed, &path),
            Some(root.path().join("installed"))
        );
        assert_eq!(
            find_uscreen_bin_in(None, earlier.join("uscreen"), &path),
            None
        );
    }

    #[test]
    fn t232_executable_discovery_without_which() {
        lookup_fixture::check(
            command_exists,
            "tests::t232_executable_discovery_without_which",
        );
    }

    #[test]
    fn t295_compatibility_url_prefills_the_actual_issue_form() {
        let url = compatibility_url("Debian & KDE / Caffè", "h264_vaapi", "Tab+Pen #2", "1.2.3");
        let (destination, query) = url.split_once('?').unwrap();
        assert_eq!(
            destination,
            "https://github.com/geraldo-netto/UScreen/issues/new"
        );
        let fields: std::collections::BTreeMap<_, _> = query
            .split('&')
            .map(|pair| pair.split_once('=').unwrap())
            .collect();
        assert_eq!(fields.get("template"), Some(&"compatibility.yml"));
        assert_eq!(fields.get("title"), Some(&"Compatibility%3A%20"));
        let form = include_str!("../../.github/ISSUE_TEMPLATE/compatibility.yml");
        let ids: Vec<_> = form
            .lines()
            .filter_map(|line| line.trim().strip_prefix("id: "))
            .collect();
        for (id, value) in [
            ("distro", "Debian%20%26%20KDE%20%2F%20Caff%C3%A8"),
            ("gpu", "h264_vaapi"),
            ("tablet", "Tab%2BPen%20%232"),
            ("version", "1.2.3"),
        ] {
            assert!(
                ids.contains(&id),
                "T295: prefill field is absent from the actual form: {id}"
            );
            assert_eq!(
                fields.get(id),
                Some(&value),
                "T295: missing or corrupt prefill for {id}"
            );
        }
        assert_eq!(
            fields.len(),
            6,
            "T295: unknown report results must stay unset"
        );
    }

    #[test]
    fn t415_linux_video_settings_show_pipe_capacity() {
        let mut app = settings_test_app(Tab::Video);
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::Grid::new("pipe-regression").show(ui, |ui| app.show_video_settings(ui));
            });
        });
        let mut text = Vec::new();
        for shape in output.shapes {
            collect_text_rects(&shape.shape, &mut text);
        }
        assert!(
            text.iter().any(|(label, _)| label == "Capture pipe buffer"),
            "T415: {text:?}"
        );
    }

    fn settings_test_app(tab: Tab) -> App {
        let saved_cfg = FileConfig::default();
        let cfg = FileConfig {
            width: saved_cfg.width + 2,
            ..saved_cfg.clone()
        };
        App {
            _status_worker: None,
            store: ConfigStore::default(),
            save: None,
            cfg,
            saved_cfg,
            tab,
            message: String::new(),
            action: None,
            status: Arc::new(Mutex::new(Status {
                daemon_running: true,
                ffmpeg_ok: true,
                adb_ok: true,
                evdi_count: 4,
                uinput_ok: true,
                ..Default::default()
            })),
            update: Arc::new(Mutex::new(None)),
        }
    }

    #[test]
    fn t409_idle_repaints_stop_while_action_completion_stays_visible() {
        let mut app = settings_test_app(Tab::Video);
        let ctx = egui::Context::default();
        let frame = |app: &mut App, time| {
            ctx.run(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| app.show_window(ctx),
            )
            .viewport_output[&egui::ViewportId::ROOT]
                .repaint_delay
        };
        for tick in 0..10 {
            frame(&mut app, tick as f64);
        }
        assert_eq!(
            frame(&mut app, 10.0),
            Duration::MAX,
            "T409: idle UI keeps scheduling repaint"
        );
        let (done, receiver) = std::sync::mpsc::channel();
        app.action = Some(receiver);
        assert!(frame(&mut app, 11.0) <= Duration::from_millis(100));
        done.send("Completed".into()).unwrap();
        frame(&mut app, 12.0);
        assert!(!app.busy());
        assert_eq!(app.message, "Completed");
    }

    #[test]
    fn t378_held_config_lock_does_not_block_apply_or_lose_edits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = ConfigStore::new(path.clone());
        store.update(|_| Ok(())).unwrap();
        let lock = std::fs::File::open(path.with_extension("lock")).unwrap();
        lock.lock().unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _ = wait.recv_timeout(Duration::from_secs(2));
            drop(lock);
        });
        let mut app = settings_test_app(Tab::Video);
        app.store = store;
        app.cfg.fps = 30;
        let started = std::time::Instant::now();
        app.apply(false);
        let elapsed = started.elapsed();
        // Stay inside the same range enforced by the real quality slider.
        app.cfg.quality = MIN_QUALITY + 1;
        app.cfg.fps = 45;
        app.apply(false); // A pending save must not enqueue another transaction.
                          // The fixture owns the lock: simulate a separate disk edit before the
                          // queued GUI transaction is allowed to read/merge it.
        let external = FileConfig {
            position: "left".into(),
            ..Default::default()
        };
        std::fs::write(path, toml::to_string(&external).unwrap()).unwrap();
        let _ = release.send(());
        holder.join().unwrap();
        assert!(
            elapsed < Duration::from_millis(200),
            "T378: apply blocked for {elapsed:?}"
        );
        wait_for_save(&mut app);
        assert_eq!(app.saved_cfg.fps, 30);
        assert_eq!(app.saved_cfg.position, "left");
        assert_eq!(
            app.cfg.quality,
            MIN_QUALITY + 1,
            "T378: edits made during save survive"
        );
        assert_eq!(app.cfg.fps, 45);
        assert_ne!(app.cfg, app.saved_cfg);
        assert_eq!(app.store.load(), app.saved_cfg);
    }

    fn wait_for_save(app: &mut App) {
        wait_for_work(app);
        assert_eq!(app.saved_cfg.fps, 30, "{}", app.message);
    }

    fn wait_for_work(app: &mut App) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while app.busy() && std::time::Instant::now() < deadline {
            app.poll_action();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.busy(), "T378: worker did not finish");
    }

    #[test]
    fn t378_save_failure_retains_baseline_and_edits_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "invalid = [").unwrap();
        let mut app = settings_test_app(Tab::Video);
        app.store = ConfigStore::new(path.clone());
        let baseline = app.saved_cfg.clone();
        app.cfg.fps = 30;
        let edits = app.cfg.clone();
        app.apply(false);
        wait_for_work(&mut app);
        assert!(app.message.starts_with("Save failed:"), "{}", app.message);
        assert_eq!(app.saved_cfg, baseline);
        assert_eq!(app.cfg, edits);
        std::fs::write(&path, "").unwrap();
        app.apply(false);
        wait_for_save(&mut app);
        assert_eq!(app.saved_cfg, app.cfg);
    }

    fn settings_test_frame(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        mut render: impl FnMut(&mut App, &mut egui::Ui),
    ) -> Vec<(String, egui::Rect)> {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::Grid::new("encoder-test").show(ui, |ui| {
                        render(app, ui);
                    });
                });
            },
        );
        let mut text = Vec::new();
        for shape in output.shapes {
            collect_text_rects(&shape.shape, &mut text);
        }
        text
    }

    fn input_test_frame(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        settings_test_frame(app, ctx, events, |app, ui| {
            app.setting_mode(ui);
            app.setting_input_devices(ui);
        })
    }

    fn window_test_frame(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.show_window(ctx),
        );
        let mut text = Vec::new();
        for shape in output.shapes {
            collect_text_rects(&shape.shape, &mut text);
        }
        text
    }

    #[test]
    fn t344_idle_input_settings_preserve_dormant_pointer_preference() {
        for pointer in [false, true] {
            let mut app = settings_test_app(Tab::Display);
            app.cfg.input_pen = false;
            app.cfg.input_pointer = pointer;
            app.saved_cfg = app.cfg.clone();
            input_test_frame(&mut app, &egui::Context::default(), Vec::new());
            assert_eq!(
                app.cfg, app.saved_cfg,
                "T344: displaying settings is not an edit"
            );
        }
    }

    #[test]
    fn t344_pen_toggle_save_reload_and_discard_preserve_pointer() {
        let root = tempfile::tempdir().unwrap();
        let mut app = settings_test_app(Tab::Display);
        app.store = ConfigStore::new(root.path().join("config.toml"));
        app.saved_cfg = app.cfg.clone();
        let ctx = egui::Context::default();
        click_settings_text(
            &mut app,
            &ctx,
            "Pen tablet (stylus, pressure, tilt)",
            input_test_frame,
        );
        assert!(!app.cfg.input_pen);
        assert!(
            app.cfg.input_pointer,
            "T344: disabling pen must retain preference"
        );
        app.apply(false);
        wait_for_work(&mut app);
        assert_eq!(app.message, "Settings saved");
        assert!(!app.store.load().input_pen);
        assert!(app.store.load().input_pointer);
        click_settings_text(
            &mut app,
            &ctx,
            "Pen tablet (stylus, pressure, tilt)",
            input_test_frame,
        );
        assert!(app.cfg.input_pen);
        assert!(app.cfg.input_pointer);
        click_settings_text(&mut app, &ctx, "Discard", window_test_frame);
        assert_eq!(app.cfg, app.saved_cfg);
        assert!(!app.cfg.input_pen);
        assert!(app.cfg.input_pointer);
    }

    fn encoder_test_frame(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        settings_test_frame(app, ctx, events, |app, ui| {
            app.setting_encoder(ui);
            app.setting_colour_depth(ui);
        })
    }

    fn pipe_test_frame(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        settings_test_frame(app, ctx, events, |app, ui| {
            let status = app.status.lock().unwrap().clone();
            pipe_settings::show(ui, &mut app.cfg.pipe_capacity_mib, &status);
        })
    }

    fn collect_text_rects(shape: &egui::Shape, text: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(value) => text.push((
                value.galley.text().to_owned(),
                egui::Rect::from_min_size(value.pos, value.galley.size()),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text_rects(shape, text);
                }
            }
            _ => {}
        }
    }

    type SettingsFrame =
        fn(&mut App, &egui::Context, Vec<egui::Event>) -> Vec<(String, egui::Rect)>;

    fn click_settings_text(app: &mut App, ctx: &egui::Context, label: &str, frame: SettingsFrame) {
        let text = frame(app, ctx, Vec::new());
        let pos = text
            .iter()
            .find(|(value, _)| value == label)
            .unwrap_or_else(|| panic!("T240: missing {label}: {text:?}"))
            .1
            .center();
        for pressed in [true, false] {
            frame(
                app,
                ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    fn click_encoder_text(app: &mut App, ctx: &egui::Context, label: &str) {
        click_settings_text(app, ctx, label, encoder_test_frame);
    }

    #[test]
    fn t415_dropdown_has_exact_choices_and_shows_effective_per_tablet_capacity() {
        let mut app = settings_test_app(Tab::Video);
        app.cfg = app.saved_cfg.clone();
        {
            let mut status = app.status.lock().unwrap();
            status.pipe_capacities = vec![(0, Some(1048576)), (1, Some(65536))];
            status.pipe_ceiling = Some(1048576);
        }
        let ctx = egui::Context::default();
        for mib in [2, 4, 8, 1] {
            let current = format!("{} MiB", app.cfg.pipe_capacity_mib);
            click_settings_text(&mut app, &ctx, &current, pipe_test_frame);
            let text = pipe_test_frame(&mut app, &ctx, vec![]);
            for label in ["1 MiB", "2 MiB", "4 MiB", "8 MiB"] {
                assert!(
                    text.iter().any(|(value, _)| value == label),
                    "T415: {text:?}"
                );
            }
            assert!(!text.iter().any(|(value, _)| value == "16 MiB"));
            click_settings_text(&mut app, &ctx, &format!("{mib} MiB"), pipe_test_frame);
            assert_eq!(app.cfg.pipe_capacity_mib, mib);
            assert!(!app.cfg.requires_restart_from(&app.saved_cfg));
        }
        click_settings_text(
            &mut app,
            &ctx,
            "How to allow larger pipes in Linux",
            pipe_test_frame,
        );
        let text = pipe_test_frame(&mut app, &ctx, vec![]);
        for expected in [
            "Tablet 1 effective capacity: 1 MiB",
            "Tablet 2 effective capacity: 64 KiB",
            "sudo sysctl -w fs.pipe-max-size=8388608",
        ] {
            assert!(
                text.iter().any(|(value, _)| value == expected),
                "T415: {text:?}"
            );
        }
    }

    #[test]
    fn t434_gui_persists_automatic_and_explicit_av1_choices() {
        let root = tempfile::tempdir().unwrap();
        let mut app = settings_test_app(Tab::Video);
        app.store = ConfigStore::new(root.path().join("config.toml"));
        let ctx = egui::Context::default();
        assert_eq!(app.cfg.encoder, "auto");
        click_encoder_text(&mut app, &ctx, "Automatic (measure compatible encoders)");
        click_encoder_text(&mut app, &ctx, "Software AV1 (libaom)");
        assert_eq!(app.cfg.encoder, "libaom-av1");
        app.apply(false);
        wait_for_work(&mut app);
        assert_eq!(app.store.load().encoder, "libaom-av1");
        click_encoder_text(&mut app, &ctx, "Software AV1 (libaom)");
        click_encoder_text(&mut app, &ctx, "Automatic (measure compatible encoders)");
        app.apply(false);
        wait_for_work(&mut app);
        assert_eq!(app.store.load().encoder, "auto");
    }

    #[test]
    fn t400_gui_persists_explicit_low_latency_vaapi_selection() {
        let root = tempfile::tempdir().unwrap();
        let mut app = settings_test_app(Tab::Video);
        app.store = ConfigStore::new(root.path().join("config.toml"));
        app.cfg.vaapi_device = "/dev/dri/renderD129".into();
        // Preserve this regression's explicit NVIDIA-to-VAAPI transition.
        app.cfg.encoder = "h264_nvenc".into();
        let ctx = egui::Context::default();
        click_encoder_text(&mut app, &ctx, "NVIDIA H.264 (NVENC)");
        click_encoder_text(&mut app, &ctx, "AMD / Intel H.264 low latency (VAAPI)");
        assert_eq!(app.cfg.encoder, "h264_vaapi_baseline");
        app.apply(false);
        wait_for_work(&mut app);
        let saved = app.store.load();
        assert_eq!(saved.encoder, "h264_vaapi_baseline");
        assert_eq!(saved.vaapi_device, "/dev/dri/renderD129");
        assert_eq!(saved.fps, uscreen_config::FileConfig::default().fps);
    }

    #[test]
    fn t240_gui_selects_hevc_vaapi_and_preserves_depth_and_device() {
        let root = tempfile::tempdir().unwrap();
        let mut app = settings_test_app(Tab::Video);
        app.store = ConfigStore::new(root.path().join("config.toml"));
        app.cfg.vaapi_device = "/dev/dri/renderD129".into();
        // Preserve this regression's explicit NVIDIA-to-VAAPI transition.
        app.cfg.encoder = "h264_nvenc".into();
        let ctx = egui::Context::default();
        click_encoder_text(&mut app, &ctx, "NVIDIA H.264 (NVENC)");
        click_encoder_text(&mut app, &ctx, "AMD / Intel HEVC (VAAPI)");
        assert_eq!(
            app.cfg.encoder, "hevc_vaapi",
            "T240: selection did not apply"
        );
        click_encoder_text(&mut app, &ctx, "10-bit (HEVC Main10)");
        assert!(app.cfg.ten_bit, "T240: HEVC depth control is disabled");
        app.apply(false);
        wait_for_work(&mut app);
        assert_eq!(app.message, "Settings saved");
        let saved = app.store.load();
        assert_eq!(saved.encoder, "hevc_vaapi");
        assert!(saved.ten_bit);
        assert_eq!(saved.vaapi_device, "/dev/dri/renderD129");
        app.cfg = saved;
        let text = encoder_test_frame(&mut app, &ctx, Vec::new());
        assert!(text
            .iter()
            .any(|(value, _)| value == "AMD / Intel HEVC (VAAPI)"));
    }

    #[test]
    fn t162_tabs_keep_one_visible_shared_apply_button() {
        for (tab, marker) in [
            (Tab::Video, "Encoder"),
            (Tab::Display, "Input devices"),
            (Tab::General, "Security"),
        ] {
            for height in [560.0, 1800.0] {
                let mut app = settings_test_app(tab);
                let ctx = egui::Context::default();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(440.0, height),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.show_window(ctx),
                );
                let mut text = Vec::new();
                for shape in output.shapes {
                    collect_text_rects(&shape.shape, &mut text);
                }
                let text = text.into_iter().map(|(value, _)| value).collect::<Vec<_>>();
                assert_eq!(
                    text.iter()
                        .filter(|line| line.as_str() == "Apply & restart")
                        .count(),
                    1
                );
                assert!(text.iter().any(|line| line == "Discard"));
                if height > 1000.0 {
                    assert!(
                        text.iter().any(|line| line == marker),
                        "missing {marker}: {text:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn t123_update_versions_follow_shared_validation_and_precedence() {
        for line in include_str!("../../testdata/version-comparisons.tsv").lines() {
            let parts: Vec<_> = line.split('\t').collect();
            assert_eq!(
                is_newer_version(parts[0], parts[1]),
                parts[2] == "true",
                "{line}"
            );
        }
    }

    #[test]
    fn t094_service_action_does_not_block_ui() {
        let start = std::time::Instant::now();
        let result = dispatch_action(|| {
            std::thread::sleep(Duration::from_millis(100));
            "done".into()
        });
        assert!(start.elapsed() < Duration::from_millis(50));
        assert_eq!(result.recv_timeout(Duration::from_secs(1)).unwrap(), "done");
    }

    mod daemon_fixture {
        include!("../../testdata/daemon_process.rs");
    }

    #[test]
    fn t436_autostart_fixture_ignores_unrelated_live_daemon() {
        let fixture = daemon_fixture::Fixture::new();
        let unrelated = fixture.start(&["start"]);
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::t231_autostart_supports_a_desktop_without_a_user_manager",
                "--nocapture",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T436: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(uscreen_config::linux::processes::Process::read(unrelated.pid()).is_some());
    }

    #[test]
    fn t231_autostart_supports_a_desktop_without_a_user_manager() {
        if std::env::var_os("USCREEN_T231_CHILD").is_some() {
            if std::env::var("USCREEN_T231_CHILD").unwrap() == "offline-enabled" {
                let path = uscreen_config::linux::autostart::desktop_path();
                let original = std::fs::read(&path).unwrap();
                assert!(
                    set_autostart_with(false, || false).is_err(),
                    "T231: unreachable enabled service was reported disabled"
                );
                assert!(autostart_enabled());
                assert_eq!(std::fs::read(path).unwrap(), original);
                return;
            }
            assert!(
                autostart_enabled(),
                "T231: enabled desktop autostart was ignored"
            );
            set_autostart_with(false, || false).unwrap();
            assert!(
                !autostart_enabled(),
                "T231: disabling autostart did not persist"
            );
            set_autostart_with(true, || false).unwrap();
            assert!(
                autostart_enabled(),
                "T231: enabling autostart did not persist"
            );
            let log = std::env::var_os("USCREEN_T231_ACTIONS").unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while std::fs::read_to_string(&log).unwrap_or_default() != "stop\nstart\n"
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                std::fs::read_to_string(log).unwrap(),
                "stop\nstart\n",
                "T231: autostart toggle lost current-daemon actions"
            );
            let routes = std::env::var_os("USCREEN_T231_ROUTES").unwrap();
            let expected = if std::env::var("USCREEN_T231_CHILD").unwrap() == "managed" {
                "managed\nmanaged\n"
            } else {
                "direct\ndirect\n"
            };
            assert_eq!(
                std::fs::read_to_string(routes).unwrap(),
                expected,
                "T436: unrelated daemon changed fixture service routing"
            );
            return;
        }
        for mode in [
            "unavailable",
            "exit127",
            "missing",
            "managed",
            "offline-enabled",
        ] {
            let sandbox = Sandbox::new();
            let home = sandbox.0.join("home");
            let config = sandbox.0.join("config space");
            std::fs::create_dir_all(config.join("autostart")).unwrap();
            std::fs::write(config.join("autostart/uscreen.desktop"),
                "[Desktop Entry]\nType=Application\nName=UScreen\nExec=uscreen start\nHidden=false\n").unwrap();
            install_t231_systemctl(&sandbox, mode);
            sandbox.script(
                "uscreen",
                "printf '%s\\n' \"$*\" >> \"$USCREEN_T231_ACTIONS\"; printf 'direct\\n' >> \"$USCREEN_T231_ROUTES\"",
            );
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::t231_autostart_supports_a_desktop_without_a_user_manager",
                    "--nocapture",
                ])
                .env("USCREEN_T231_CHILD", mode)
                .env("HOME", &home)
                .env("XDG_CONFIG_HOME", &config)
                .env("USCREEN_T231_ACTIONS", sandbox.0.join("actions"))
                .env("USCREEN_T231_ROUTES", sandbox.0.join("routes"))
                .env("USCREEN_T231_STATE", sandbox.0.join("enabled"))
                .env("PATH", &sandbox.0)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "T231 systemctl {mode}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    fn install_t231_systemctl(sandbox: &Sandbox, mode: &str) {
        let script = match mode {
            "missing" => return,
            "exit127" => "exit 127",
            "offline-enabled" => "if [ \"$2\" = is-enabled ]; then echo enabled; else exit 1; fi",
            "managed" => {
                r#"case "$2" in
show) echo loaded ;;
is-active) exit 1 ;;
is-enabled) [ -f "$USCREEN_T231_STATE" ] && echo enabled ;;
enable) : > "$USCREEN_T231_STATE" ;;
disable) /bin/rm -f "$USCREEN_T231_STATE" ;;
start|stop) printf '%s\n' "$2" >> "$USCREEN_T231_ACTIONS"; printf 'managed\n' >> "$USCREEN_T231_ROUTES" ;;
*) exit 99 ;;
esac"#
            }
            _ => "exit 1",
        };
        sandbox.script("systemctl", script);
    }

    #[test]
    fn t260_gui_recovers_daemons_and_routes_actions_without_trusting_pid_files() {
        if let Some(pid) = std::env::var_os("USCREEN_T260_DAEMON") {
            check_t260_gui_state(pid.to_str().unwrap().parse().unwrap());
            return;
        }
        let fixture = daemon_fixture::Fixture::new();
        let runtime = fixture.root.path().join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        let daemon = fixture.start_named("uscreen", &[]);
        let diagnostic = fixture.start(&["doctor"]);
        let sandbox = Sandbox::new();
        sandbox.script("adb", "exit 0");
        sandbox.script("systemctl", "case \"$2\" in is-active) exit 1;; show) echo loaded;; is-enabled) echo disabled;; *) exit 99;; esac");
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::t260_gui_recovers_daemons_and_routes_actions_without_trusting_pid_files",
                "--nocapture",
            ])
            .env("USCREEN_T260_DAEMON", daemon.pid().to_string())
            .env("USCREEN_T260_DIAGNOSTIC", diagnostic.pid().to_string())
            .env("HOME", fixture.root.path())
            .env("XDG_RUNTIME_DIR", runtime)
            .env("PATH", &sandbox.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T260: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn check_t260_gui_state(daemon: u32) {
        let diagnostic: u32 = std::env::var("USCREEN_T260_DIAGNOSTIC")
            .unwrap()
            .parse()
            .unwrap();
        std::fs::create_dir_all(pid_path().parent().unwrap()).unwrap();
        for stale in [
            None,
            Some("0".into()),
            Some("4294967295".into()),
            Some("invalid".into()),
            Some(diagnostic.to_string()),
            Some(daemon.to_string()),
        ] {
            let _ = std::fs::remove_file(pid_path());
            if let Some(text) = &stale {
                std::fs::write(pid_path(), text).unwrap();
            }
            let status = poll_status();
            assert!(
                status.daemon_running,
                "T260: live daemon reported stopped with PID file {stale:?}"
            );
            if stale.as_deref() == Some(daemon.to_string().as_str()) {
                assert_eq!(
                    status.daemon_pid, daemon,
                    "T260: valid tracked daemon lost priority"
                );
            }
            assert_ne!(
                status.daemon_pid, diagnostic,
                "T260: doctor is not a daemon"
            );
            assert!(
                !service_managed(),
                "T260: inactive installed service captured direct-daemon actions"
            );
            let command = daemon_command(
                std::path::Path::new("/fixture/uscreen"),
                "stop",
                service_managed(),
            );
            assert_eq!(command.get_program(), "/fixture/uscreen");
            assert_eq!(command.get_args().collect::<Vec<_>>(), ["stop"]);
        }
    }

    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "uscreen-gui-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn script(&self, name: &str, body: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn t014_packaged_daemon_wins_over_stale_local_install() {
        let sandbox = Sandbox::new();
        let sibling = sandbox.script("package/uscreen", "exit 0");
        let local = sandbox.script("local/uscreen", "exit 0");
        let path_bin = sandbox.script("path/uscreen", "exit 0");
        let path = path_bin.parent().unwrap().as_os_str();
        assert_eq!(
            find_uscreen_bin_in(
                Some(sibling.with_file_name("uscreen-gui")),
                local.clone(),
                path
            ),
            Some(sibling.clone())
        );
        std::fs::remove_file(sibling).unwrap();
        assert_eq!(
            find_uscreen_bin_in(None, local.clone(), path),
            Some(path_bin.clone())
        );
        std::fs::remove_file(&path_bin).unwrap();
        assert_eq!(find_uscreen_bin_in(None, local.clone(), path), Some(local));
    }

    #[test]
    fn t135_tablet_status_uses_live_assignments_and_all_models() {
        let sandbox = Sandbox::new();
        let executable = sandbox.0.join("uscreen");
        std::os::unix::fs::symlink("/bin/sleep", &executable).unwrap();
        let mut daemon = Command::new(&executable).arg("30").spawn().unwrap();
        let pid = daemon.id();
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let start = stat
            .rsplit_once(") ")
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap();
        let sessions = sandbox.0.join("sessions.json");
        let devices = "List of devices attached\nPHONE device model:Charging_Phone\n192.0.2.10:5555 device model:WiFi_Tablet\nUSB_TABLET device model:Second_Tablet\n";
        let snapshot = format!(
            r#"{{"pid":{pid},"start_ticks":{start},"sessions":[{{"serial":"192.0.2.10:5555","instance":0,"video_port":19000,"input_port":20000}},{{"serial":"USB_TABLET","instance":1,"video_port":19002,"input_port":20002}}]}}"#
        );
        std::fs::write(&sessions, snapshot).unwrap();
        let mut status = Status::default();
        apply_tablet_status(&mut status, devices, Some(&sessions));
        // Reap before assertions so a failing regression leaves no test process.
        daemon.kill().unwrap();
        daemon.wait().unwrap();
        assert!(status.tablet_connected);
        assert_eq!(status.tablet_model, "WiFi Tablet, Second Tablet");
        let mut stale = Status::default();
        apply_tablet_status(&mut stale, devices, Some(&sessions));
        assert!(
            !stale.tablet_connected,
            "dead daemon ledger cannot claim active tablets"
        );
    }

    #[test]
    fn t135_missing_sessions_do_not_claim_a_charging_phone() {
        let sandbox = Sandbox::new();
        let mut status = Status::default();
        apply_tablet_status(
            &mut status,
            "List of devices attached\nPHONE device model:Phone\n",
            Some(&sandbox.0.join("missing.json")),
        );
        assert!(!status.tablet_connected);
        assert!(status.tablet_model.is_empty());
    }

    #[test]
    fn t328_setup_timeout_reports_continued_work() {
        let result = system_setup_result(
            Command::new("sh").args(["-c", "exec sleep 1"]),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(result.contains("Setup may still be running"), "{result}");
        assert!(!result.contains("failed to run"), "{result}");
    }

    #[test]
    fn t015_system_setup_installs_uinput_rule_and_reloads_udev() {
        let sandbox = Sandbox::new();
        for dir in [
            "etc/modprobe.d",
            "etc/modules-load.d",
            "sys/devices/evdi",
            "etc/udev/rules.d",
        ] {
            std::fs::create_dir_all(sandbox.0.join(dir)).unwrap();
        }
        std::fs::write(sandbox.0.join("sys/devices/evdi/count"), "2").unwrap();
        sandbox.script("bin/modprobe", "exit 0");
        sandbox.script(
            "bin/udevadm",
            "printf '%s\\n' \"$*\" >> \"$USCREEN_TEST_TRACE\"",
        );
        let trace = sandbox.0.join("trace");
        let output = Command::new("sh")
            .args(["-c", &system_setup_script(&sandbox.0, 2)])
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", sandbox.0.join("bin").display()),
            )
            .env("USCREEN_TEST_TRACE", &trace)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rule =
            std::fs::read_to_string(sandbox.0.join("etc/udev/rules.d/60-uscreen-uinput.rules"));
        assert_eq!(
            rule.unwrap(),
            include_str!("../../packaging/60-uscreen-uinput.rules")
        );
        let trace = std::fs::read_to_string(trace).unwrap();
        assert!(trace.contains("control --reload"));
        assert!(trace.contains("trigger --name-match=uinput"));
    }

    #[test]
    fn t136_setup_readiness_requires_all_configured_tablets() {
        let status = Status {
            evdi_count: 2,
            uinput_ok: true,
            ..Default::default()
        };
        assert!(needs_system_setup(
            &status,
            &FileConfig {
                max_tablets: 4,
                ..Default::default()
            }
        ));
        assert!(!needs_system_setup(
            &status,
            &FileConfig {
                max_tablets: 2,
                ..Default::default()
            }
        ));
    }

    #[test]
    fn t136_setup_adds_missing_devices_and_persists_capacity() {
        for (existing, wanted) in [(2, 4), (0, 3), (4, 2)] {
            let sandbox = Sandbox::new();
            std::fs::create_dir_all(sandbox.0.join("sys/devices/evdi")).unwrap();
            std::fs::write(
                sandbox.0.join("sys/devices/evdi/count"),
                existing.to_string(),
            )
            .unwrap();
            sandbox.script("bin/modprobe", "exit 0");
            sandbox.script("bin/udevadm", "exit 0");
            let output = Command::new("sh")
                .args(["-c", &system_setup_script(&sandbox.0, wanted)])
                .env(
                    "PATH",
                    format!("{}:/usr/bin:/bin", sandbox.0.join("bin").display()),
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let boot = std::fs::read_to_string(sandbox.0.join("etc/modprobe.d/uscreen-evdi.conf"))
                .unwrap();
            assert_eq!(
                boot.trim(),
                format!("options evdi initial_device_count={wanted}")
            );
            let added = std::fs::read_to_string(sandbox.0.join("sys/devices/evdi/add"));
            if wanted > existing {
                assert_eq!(added.unwrap().trim(), (wanted - existing).to_string());
            } else {
                assert!(added.is_err(), "must preserve existing active devices");
            }
        }
    }

    #[test]
    fn t097_gui_setup_creates_missing_configuration_directories() {
        let sandbox = Sandbox::new();
        std::fs::create_dir_all(sandbox.0.join("sys/devices/evdi")).unwrap();
        std::fs::write(sandbox.0.join("sys/devices/evdi/count"), "2").unwrap();
        sandbox.script("bin/modprobe", "exit 0");
        sandbox.script("bin/udevadm", "exit 0");
        let output = Command::new("sh")
            .args(["-c", &system_setup_script(&sandbox.0, 2)])
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", sandbox.0.join("bin").display()),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for file in [
            "etc/modprobe.d/uscreen-evdi.conf",
            "etc/modules-load.d/uscreen.conf",
            "etc/udev/rules.d/60-uscreen-uinput.rules",
        ] {
            assert!(sandbox.0.join(file).is_file(), "missing {file}");
        }
    }

    #[test]
    fn t015_unwritable_uinput_is_visible_even_when_evdi_is_ready() {
        let mut status = Status {
            evdi_count: 2,
            uinput_ok: false,
            ..Default::default()
        };
        assert!(needs_system_setup(&status, &FileConfig::default()));
        status.uinput_ok = true;
        assert!(!needs_system_setup(&status, &FileConfig::default()));
        status.uinput_ok = false;
        assert!(!needs_system_setup(
            &status,
            &FileConfig {
                input_touch: false,
                input_pen: false,
                ..Default::default()
            }
        ));
    }

    #[test]
    fn t268_slow_valid_stop_completes_before_restart() {
        for managed in [false, true] {
            let sandbox = Sandbox::new();
            let command = sandbox.script("slow-lifecycle", "sleep 6; echo completed");
            let restarted = std::cell::Cell::new(false);
            let before = std::time::Instant::now();
            let result = restart_with(
                managed,
                |action, through_service| {
                    assert_eq!(through_service, managed);
                    assert_eq!(action, if managed { "restart" } else { "stop" });
                    execute_daemon_command(action, &mut Command::new(&command), through_service)?;
                    restarted.set(managed);
                    Ok(())
                },
                || {
                    restarted.set(true);
                    Ok(())
                },
            );
            assert!(
                result.is_ok(),
                "T268: valid cleanup was interrupted: {result:?}"
            );
            assert!(
                restarted.get(),
                "T268: restart abandoned after a valid stop"
            );
            assert!(before.elapsed() >= Duration::from_secs(6));
        }
    }

    #[test]
    fn t268_over_budget_stop_is_reaped_and_prevents_restart() {
        let sandbox = Sandbox::new();
        let pid_file = sandbox.0.join("pid");
        let program = sandbox.script(
            "stuck-stop",
            r#"echo $$ > "$USCREEN_T268_PID"; exec sleep 60"#,
        );
        let restarted = std::cell::Cell::new(false);
        let before = std::time::Instant::now();
        let result = restart_with(
            false,
            |action, managed| {
                execute_daemon_command(
                    action,
                    Command::new(&program).env("USCREEN_T268_PID", &pid_file),
                    managed,
                )
            },
            || {
                restarted.set(true);
                Ok(())
            },
        );
        assert!(result.unwrap_err().contains("timed out"));
        assert!(
            !restarted.get(),
            "T268: never overlap start with failed stop"
        );
        let elapsed = before.elapsed();
        assert!(elapsed >= daemon_command_timeout(false));
        assert!(elapsed < daemon_command_timeout(false) + Duration::from_secs(2));
        let pid = std::fs::read_to_string(pid_file).unwrap();
        assert!(
            !std::path::Path::new(&format!("/proc/{}", pid.trim())).exists(),
            "T268: timed out command not reaped"
        );
    }

    #[test]
    fn t016_service_actions_use_systemd_including_restart() {
        let sandbox = Sandbox::new();
        let bin = sandbox.script("uscreen", "echo direct");
        sandbox.script("systemctl", "printf 'managed %s\\n' \"$*\"");
        for action in ["start", "stop", "restart"] {
            let output = daemon_command(&bin, action, true)
                .env("PATH", &sandbox.0)
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                format!("managed --user {action} uscreen.service\n")
            );
        }
        let output = daemon_command(&bin, "stop", false).output().unwrap();
        assert_eq!(output.stdout, b"direct\n");
    }

    #[test]
    fn t053_slider_can_select_the_supported_one_mbps_minimum() {
        let ctx = egui::Context::default();
        let mut bitrate = 20_000;
        let mut rect = egui::Rect::NOTHING;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                rect = bitrate_slider(ui, &mut bitrate).rect;
            });
        });
        let pos = egui::pos2(rect.left() + 1.0, rect.center().y);
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                bitrate_slider(ui, &mut bitrate);
            });
        });
        assert_eq!(bitrate, 1000);
    }

    #[test]
    fn t053_displaying_bitrate_slider_does_not_rewrite_supported_low_values() {
        for original in [1000, 2500, 4999, 5000, 60000] {
            let mut bitrate = original;
            let ctx = egui::Context::default();
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| bitrate_slider(ui, &mut bitrate));
            });
            assert_eq!(bitrate, original);
        }
    }
}
