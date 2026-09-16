#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uscreen_config::commands::SyncCommandExt;
use uscreen_config::*;

/// Whether the systemd user service is enabled, i.e. whether plugging the
/// cable in is enough on its own.
fn autostart_enabled() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "uscreen.service"])
        .output_bounded()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(false)
}

fn set_autostart(on: bool) -> Result<(), String> {
    let verb = if on { "enable" } else { "disable" };
    let out = Command::new("systemctl")
        .args(["--user", verb, "--now", "uscreen.service"])
        .output_bounded()
        .map_err(|e| format!("systemctl failed: {}", e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[derive(Default, Clone)]
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
    if let Some(sibling) = exe.and_then(|exe| exe.parent().map(|dir| dir.join("uscreen"))) {
        if sibling.exists() {
            return Some(sibling);
        }
    }
    std::env::split_paths(path)
        .map(|dir| dir.join("uscreen"))
        .find(|p| p.is_file())
        .or_else(|| installed.is_file().then_some(installed))
}

fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output_bounded()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[allow(clippy::field_reassign_with_default)]
fn poll_status() -> Status {
    let mut s = Status::default();

    s.evdi_count = std::fs::read_to_string("/sys/devices/evdi/count")
        .ok()
        .and_then(|t| t.trim().parse::<i32>().ok())
        .unwrap_or(-1);
    s.ffmpeg_ok = command_exists("ffmpeg");
    s.autostart = autostart_enabled();
    s.adb_ok = command_exists("adb");
    s.uinput_ok = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .is_ok();

    if let Ok(pid_str) = std::fs::read_to_string(pid_path()) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if daemon_is_running(pid) {
                s.daemon_running = true;
                s.daemon_pid = pid;
            }
        }
    }

    // Not `adb get-state`: it fails outright as soon as two devices are
    // reachable, which is the normal state with `adb tcpip` in use - the
    // window would have said "no tablet" while the daemon was streaming.
    if let Ok(out) = Command::new("adb").args(["devices", "-l"]).output_bounded() {
        let text = String::from_utf8_lossy(&out.stdout);
        let ready: Vec<&str> = text
            .lines()
            .skip(1)
            .filter(|l| l.split_whitespace().nth(1) == Some("device"))
            .collect();
        // Prefer the USB entry (no colon in the serial), like the daemon does.
        if let Some(line) = ready
            .iter()
            .find(|l| !l.split_whitespace().next().unwrap_or("").contains(':'))
            .or(ready.first())
        {
            s.tablet_connected = true;
            if let Some(model) = line
                .split_whitespace()
                .find_map(|t| t.strip_prefix("model:"))
            {
                s.tablet_model = model.replace('_', " ");
            }
        }
    }
    s
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
    let out = Command::new("pkexec")
        .args(["sh", "-c", &script])
        .output_timeout(Duration::from_secs(120))
        .map_err(|e| format!("pkexec failed to run: {}", e))?;
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
    if Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "uscreen.service"])
        .output_bounded()
        .is_ok_and(|output| output.status.success())
    {
        return true;
    }
    let running_directly = std::fs::read_to_string(pid_path())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .is_some_and(daemon_is_running);
    if running_directly {
        return false;
    }
    Command::new("systemctl")
        .args([
            "--user",
            "show",
            "-p",
            "LoadState",
            "--value",
            "uscreen.service",
        ])
        .output_bounded()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "loaded")
}

fn run_daemon_command(action: &str, managed: bool) -> Result<(), String> {
    let bin = if managed {
        PathBuf::new()
    } else {
        find_uscreen_bin().ok_or("uscreen binary not found")?
    };
    let output = daemon_command(&bin, action, managed)
        .output_bounded()
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
    if service_managed() {
        return run_daemon_command("restart", true);
    }
    run_daemon_command("stop", false)?;
    start_direct_daemon()
}

fn start_daemon() -> Result<(), String> {
    if service_managed() {
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

const RELEASES_API: &str = "https://api.github.com/repos/majmichu1/UScreen/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/majmichu1/UScreen/releases/latest";

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
    let body = String::from_utf8_lossy(&out.stdout);
    let tag = body.split("\"tag_name\"").nth(1)?.split('"').nth(1)?;
    is_newer_version(tag, env!("CARGO_PKG_VERSION")).then(|| {
        tag.trim()
            .strip_prefix('v')
            .unwrap_or(tag.trim())
            .to_owned()
    })
}

impl App {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = FileConfig::load();
        let status = Arc::new(Mutex::new(Status::default()));

        let update = Arc::new(Mutex::new(None));
        if cfg.check_updates {
            let slot = update.clone();
            std::thread::spawn(move || {
                if let Some(v) = check_for_update() {
                    if let Ok(mut g) = slot.lock() {
                        *g = Some(v);
                    }
                }
            });
        }

        // Background poller: daemon + adb state every 2 seconds
        let status_bg = status.clone();
        std::thread::spawn(move || loop {
            let s = poll_status();
            if let Ok(mut guard) = status_bg.lock() {
                *guard = s;
            }
            std::thread::sleep(Duration::from_secs(2));
        });

        Self {
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
        if self.action.is_none() {
            self.message = "Working…".into();
            self.action = Some(dispatch_action(action));
        }
    }

    fn apply(&mut self, restart: bool) {
        if self.action.is_some() {
            return;
        }
        match self.cfg.save_edits(&self.saved_cfg) {
            Ok(merged) => {
                self.cfg = merged.clone();
                self.saved_cfg = merged;
                self.message = "Settings saved".into();
                if restart {
                    self.run_action(|| {
                        restart_daemon()
                            .map(|_| "Settings saved — daemon restarted".into())
                            .unwrap_or_else(|e| e)
                    });
                }
            }
            Err(e) => self.message = format!("Save failed: {}", e),
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
        ctx.request_repaint_after(Duration::from_secs(1));
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
        let status = self.status.lock().map(|s| s.clone()).unwrap_or_default();

        // Keep one shared action row below every tab, visible while settings scroll.
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.add_space(8.0);
            let dirty = self.cfg != self.saved_cfg;
            ui.horizontal(|ui| {
                let label = if status.daemon_running {
                    "Apply & restart"
                } else {
                    "Save"
                };
                if ui.add_enabled(dirty, egui::Button::new(label)).clicked() {
                    self.apply(status.daemon_running);
                }
                if dirty && ui.button("Discard").clicked() {
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
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                ui.add_space(6.0);
                ui.heading(egui::RichText::new("UScreen").size(26.0));
                ui.label(egui::RichText::new("USB second display for your tablet").weak());
                ui.horizontal(|ui| {
                    if ui.small_button("Report compatibility").on_hover_text(
                        "Opens a GitHub issue pre-filled with your setup. Nothing is sent until you submit it.").clicked()
                    {
                        let body = format!(
                            "Result: \n\nDistribution and desktop: {}\nGPU and encoder: {}\nTablet, Android, stylus: {}\nUScreen version: {}\n\nLatency line from the log (optional):\n\nNotes:\n",
                            os_release_name(), self.cfg.encoder, status.tablet_model, env!("CARGO_PKG_VERSION"));
                        let url = format!(
                            "https://github.com/majmichu1/UScreen/issues/new?template=compatibility.yml&title={}&body={}",
                            urlencode("Compatibility: "), urlencode(&body));
                        let _ = spawn_reaped(Command::new("xdg-open").arg(url));
                    }
                    if ui.small_button("Star on GitHub").clicked() {
                        let _ = spawn_reaped(Command::new("xdg-open").arg("https://github.com/majmichu1/UScreen"));
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

                // ----- First-run system setup -----
                let needs_setup = needs_system_setup(&status, &self.cfg);
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
                            if needs_setup {
                                ui.label(if status.evdi_count < 0 {
                                    "The EVDI kernel module is not loaded (install evdi/evdi-dkms)."
                                } else if status.evdi_count < self.cfg.max_tablets as i32 {
                                    "More virtual display devices are needed for the configured tablet count."
                                } else {
                                    "Touch and pen input need permission to access /dev/uinput."
                                });
                                if ui.button("Set up display and input (asks for password)").clicked()
                                {
                                    let max_tablets = self.cfg.max_tablets;
                                    self.run_action(move || run_system_setup(max_tablets).map(|_| "System setup complete".into()).unwrap_or_else(|e| e));
                                }
                            }
                        });
                    ui.add_space(10.0);
                }

                // ----- Status -----
                egui::Frame::group(ui.style())
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        status_dot(
                            ui,
                            status.daemon_running,
                            if status.daemon_running { "Daemon running" } else { "Daemon stopped" },
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
                            if status.tablet_connected { "Tablet connected" } else { "No tablet detected" },
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

                // ----- Start / Stop -----
                ui.horizontal(|ui| {
                    let big = egui::vec2(ui.available_width(), 34.0);
                    if status.daemon_running {
                        if ui
                            .add_sized(big, egui::Button::new(egui::RichText::new("Stop").size(16.0)))
                            .clicked()
                        {
                            self.run_action(|| stop_daemon().map(|_| "Daemon stopped".into()).unwrap_or_else(|e| e));
                        }
                    } else if ui
                        .add_sized(big, egui::Button::new(egui::RichText::new("Start").size(16.0)))
                        .clicked()
                    {
                        self.run_action(|| start_daemon().map(|_| "Daemon starting…".into()).unwrap_or_else(|e| e));
                    }
                });

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);

                // ----- Settings -----
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

                // One grid per tab: column widths are remembered by id, and
                // the tabs have different label widths.
                egui::Grid::new(("settings", self.tab))
                    .num_columns(2)
                    .spacing([16.0, 10.0])
                    .show(ui, |ui| {
                        if self.tab == Tab::Video {
                            ui.label("Encoder");
                            egui::ComboBox::from_id_salt("encoder")
                                .selected_text(match self.cfg.encoder.as_str() {
                                    "h264_nvenc" => "NVIDIA H.264 (NVENC)",
                                    "hevc_nvenc" => "NVIDIA HEVC (NVENC)",
                                    "h264_vaapi" | "vaapih264enc" => "AMD / Intel (VAAPI)",
                                    "libx264" => "CPU (libx264)",
                                    other => other,
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.cfg.encoder,
                                        "h264_nvenc".to_string(),
                                        "NVIDIA H.264 (NVENC)",
                                    );
                                    ui.selectable_value(
                                        &mut self.cfg.encoder,
                                        "hevc_nvenc".to_string(),
                                        "NVIDIA HEVC (NVENC)",
                                    );
                                    ui.selectable_value(
                                        &mut self.cfg.encoder,
                                        "h264_vaapi".to_string(),
                                        "AMD / Intel (VAAPI)",
                                    );
                                    ui.selectable_value(
                                        &mut self.cfg.encoder,
                                        "libx264".to_string(),
                                        "CPU (libx264)",
                                    );
                                });
                            ui.end_row();
                        }

                        if self.tab == Tab::Video {
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
                                    egui::RichText::new(
                                        "This, not the bitrate, sets how sharp text looks",
                                    )
                                    .weak()
                                    .size(11.0),
                                );
                            });
                            ui.end_row();
                        }

                        if self.tab == Tab::Video {
                            ui.label("Bitrate ceiling");
                            ui.vertical(|ui| {
                                bitrate_slider(ui, &mut self.cfg.bitrate);
                                ui.label(
                                    egui::RichText::new(
                                        "Only a cap for bursts — a desktop streams well below it",
                                    )
                                    .weak()
                                    .size(11.0),
                                );
                            });
                            ui.end_row();
                        }

                        if self.tab == Tab::Video {
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

                        if self.tab == Tab::General {
                            ui.label("Security");
                            ui.vertical(|ui| {
                                ui.checkbox(&mut self.cfg.require_token, "Require the session token");
                                ui.label(egui::RichText::new(
                                    "Off only for an app older than 1.1.0. Without it any local process \
                                     can read the screen and inject input.").small().weak());
                            });
                            ui.end_row();
                        }

                        if self.tab == Tab::General {
                            ui.label("Updates");
                            ui.checkbox(&mut self.cfg.check_updates, "Check for a newer release on start");
                            ui.end_row();
                        }

                        if self.tab == Tab::Display {
                            ui.label("Tablets");
                            ui.vertical(|ui| {
                                ui.add(egui::Slider::new(&mut self.cfg.max_tablets, 1..=4).text("at once"));
                                ui.label(egui::RichText::new(
                                    "Each tablet becomes its own screen. Needs that many EVDI devices \
                                     (see uscreen doctor); the installer prepares two.").small().weak());
                            });
                            ui.end_row();
                        }

                        if self.tab == Tab::Display {
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

                        if self.tab == Tab::Video {
                            ui.label("Colour depth");
                            ui.vertical(|ui| {
                                let hevc = self.cfg.encoder.contains("hevc");
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

                        if self.tab == Tab::Video {
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

                        if self.tab == Tab::Display {
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

                        if self.tab == Tab::Display {
                            ui.label("Input devices");
                            ui.vertical(|ui| {
                                ui.checkbox(&mut self.cfg.input_touch, "Touchscreen (taps on the tablet)");
                                ui.checkbox(&mut self.cfg.input_pen, "Pen tablet (stylus, pressure, tilt)");
                                // The pointer exists only to serve the pen; a greyed-out
                                // box must not keep a value the daemon would act on.
                                if !self.cfg.input_pen {
                                    self.cfg.input_pointer = false;
                                }
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

                        if self.tab == Tab::Video {
                            ui.label("Stream detail");
                            ui.vertical(|ui| {
                                egui::ComboBox::from_id_salt("stream_scale")
                                    .selected_text(match self.cfg.stream_scale {
                                        1 => "Full — sharpest",
                                        2 => "Half — lowest latency",
                                        n => scale_label(n),
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.cfg.stream_scale, 1,
                                            "Full — sharpest");
                                        ui.selectable_value(&mut self.cfg.stream_scale, 2,
                                            "Half — lowest latency");
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

                        if self.tab == Tab::General {
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
                                    self.run_action(move || set_autostart(auto).map(|_| {
                                        if auto { "Autostart on — plugging the cable in is now enough".into() }
                                        else { "Autostart off".into() }
                                    }).unwrap_or_else(|e| e));
                                }
                            });
                            ui.end_row();
                        }
                    });

            });
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
