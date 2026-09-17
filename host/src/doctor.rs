//! `uscreen doctor` — one-shot check of everything that has to be true for the
//! pipeline to work, with an actionable hint for every failure.
//!
//! This is deliberately the first thing to run whenever the stream "is laggy
//! again". The most common cause is not the pipeline at all but orphaned
//! `evdi_helper`/`ffmpeg` processes from a previous run: several writers on the
//! shared capture FIFO interleave at pipe granularity. Diagnostics identify
//! the affected owned pipeline; capture startup retires matching processes
//! before attaching a replacement helper.

use crate::capture::fifo_path_for;
use crate::config::{self, FileConfig, MAX_BITRATE_KBPS, MAX_FPS, MIN_BITRATE_KBPS, MIN_FPS};
use crate::vdisplay;
use anyhow::Result;
use std::path::Path;
use uscreen_config::adb::{transport_of, Transport};
use uscreen_config::commands::AsyncCommandExt;
use uscreen_config::linux::processes::{self, CaptureRole, Process};

#[derive(PartialEq)]
enum Level {
    Ok,
    Warn,
    Fail,
}

struct Report {
    warnings: u32,
    failures: u32,
    #[cfg(test)]
    messages: std::cell::RefCell<Vec<String>>,
}

impl Report {
    fn new() -> Self {
        Self {
            warnings: 0,
            failures: 0,
            #[cfg(test)]
            messages: Default::default(),
        }
    }

    fn line(&mut self, level: Level, label: &str, detail: &str) {
        #[cfg(test)]
        self.messages
            .borrow_mut()
            .push(format!("{label}: {detail}"));
        let mark = match level {
            Level::Ok => "  ok  ",
            Level::Warn => " warn ",
            Level::Fail => " FAIL ",
        };
        match level {
            Level::Warn => self.warnings += 1,
            Level::Fail => self.failures += 1,
            Level::Ok => {}
        }
        if detail.is_empty() {
            println!("[{}] {}", mark, label);
        } else {
            println!("[{}] {:<34} {}", mark, label, detail);
        }
    }

    /// A remedy printed under the finding it belongs to.
    fn hint(&self, text: &str) {
        #[cfg(test)]
        self.messages.borrow_mut().push(text.into());
        println!("         → {}", text);
    }
}

fn section(title: &str) {
    println!("\n{}", title);
    println!("{}", "-".repeat(title.len()));
}

async fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new(program)
        .args(args)
        .output_bounded()
        .await
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

use uscreen_config::linux::programs::command_exists;

fn check_modules(r: &mut Report, cfg: &FileConfig) {
    match std::fs::read_to_string("/sys/devices/evdi/count") {
        Ok(text) => {
            let count: i32 = text.trim().parse().unwrap_or(-1);
            if count > 0 {
                r.line(Level::Ok, "evdi module", &format!("{} device(s)", count));
            } else {
                r.line(Level::Fail, "evdi module", "loaded, but no device created");
                r.hint("for this boot:   echo 1 | sudo tee /sys/devices/evdi/add");
                // The one-shot write above does not survive a reboot, and
                // initial_device_count is only read when the module loads —
                // so writing the modprobe.d file is not enough on a machine
                // where evdi is already resident. Both halves matter.
                r.hint(
                    "for every boot: echo 'options evdi initial_device_count=2' | sudo tee \
                     /etc/modprobe.d/uscreen-evdi.conf # takes effect after reboot; keep the live module loaded",
                );
            }
        }
        Err(_) => {
            r.line(Level::Fail, "evdi module", "not loaded");
            r.hint("install evdi-dkms (on Arch it is in the AUR: yay -S evdi-dkms), then: sudo modprobe evdi");
        }
    }

    let uinput = Path::new("/dev/uinput");
    // With every input device switched off the daemon never opens uinput,
    // so a problem here is worth knowing about but blocks nothing.
    let uinput_level = if cfg.input_touch || cfg.input_pen {
        Level::Fail
    } else {
        Level::Warn
    };
    if !uinput.exists() {
        r.line(uinput_level, "uinput device", "/dev/uinput missing");
        r.hint("sudo modprobe uinput");
    } else {
        // Existence is not enough — the daemon runs unprivileged and needs to
        // open it for writing, which is what actually fails in practice.
        match std::fs::OpenOptions::new().write(true).open(uinput) {
            Ok(_) => r.line(Level::Ok, "uinput device", "writable"),
            Err(e) => {
                r.line(
                    uinput_level,
                    "uinput device",
                    &format!("not writable: {}", e),
                );
                r.hint(&uinput_hint(Path::new("/")));
            }
        }
    }
}

fn uinput_hint(root: &Path) -> String {
    if ["etc/udev/rules.d", "usr/lib/udev/rules.d"]
        .iter()
        .any(|dir| root.join(dir).join("60-uscreen-uinput.rules").exists())
    {
        "rule installed: sudo udevadm control --reload && sudo udevadm trigger --name-match=uinput; log in locally at an active seat to receive the uaccess grant".into()
    } else {
        "reinstall the UScreen package or rerun the release tarball's scripts/install.sh to install the uinput rule".into()
    }
}

async fn check_tools(r: &mut Report, cfg: &FileConfig) {
    // An empty path makes the existing execution probe report a missing helper.
    let helper = crate::find_helper(None).unwrap_or_default();
    check_tools_with_helper(r, cfg, &helper).await;
}

async fn check_tools_with_helper(r: &mut Report, cfg: &FileConfig, helper: &Path) {
    check_helper_execution(r, helper).await;
    check_required_commands(r);
    check_encoder_availability(r, cfg).await;
}

async fn check_helper_execution(r: &mut Report, helper: &Path) {
    // With no arguments the helper prints usage and exits before opening EVDI.
    match tokio::process::Command::new(helper).output_bounded().await {
        Ok(out)
            if out.status.success()
                || (out.status.code() == Some(1)
                    && String::from_utf8_lossy(&out.stderr).contains("Usage:")) =>
        {
            r.line(Level::Ok, "evdi_helper", &helper.display().to_string())
        }
        Ok(out) => {
            r.line(
                Level::Fail,
                "evdi_helper",
                &format!(
                    "could not execute normally: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            );
            r.hint("reinstall UScreen and the libevdi runtime package for this distribution");
        }
        Err(e) => {
            r.line(
                Level::Fail,
                "evdi_helper",
                &format!("could not execute {}: {e}", helper.display()),
            );
            r.hint("reinstall UScreen (including evdi_helper) and its libevdi runtime dependency");
        }
    }
}

fn check_required_commands(r: &mut Report) {
    for (tool, fatal) in [("ffmpeg", true), ("adb", true), ("kscreen-doctor", false)] {
        if command_exists(tool) {
            r.line(Level::Ok, tool, "found");
        } else if fatal {
            r.line(Level::Fail, tool, "not installed");
            r.hint(&format!("install {} with your package manager", tool));
        } else {
            r.line(Level::Warn, tool, "not installed (KDE only)");
        }
    }
}

async fn check_encoder_availability(r: &mut Report, cfg: &FileConfig) {
    if let Some(list) = output_of("ffmpeg", &["-hide_banner", "-encoders"]).await {
        report_encoder_availability(r, cfg, &list);
    }
}

fn report_encoder_availability(r: &mut Report, cfg: &FileConfig, list: &str) {
    if let Err(error) = config::validate_encoder_for_build(&cfg.encoder) {
        r.line(Level::Fail, "configured encoder", &error.to_string());
        return;
    }
    let has = |name: &str| {
        list.lines()
            .any(|l| l.split_whitespace().any(|t| t == name))
    };
    if has(config::ffmpeg_encoder_name(&cfg.encoder)) {
        r.line(
            Level::Ok,
            "configured encoder",
            &format!("{} available", cfg.encoder),
        );
    } else {
        r.line(
            Level::Fail,
            "configured encoder",
            &format!("{} NOT available in ffmpeg", cfg.encoder),
        );
        let alternatives: Vec<&str> = ["h264_nvenc", "h264_vaapi", "libx264"]
            .into_iter()
            .filter(|e| has(e))
            .collect();
        r.hint(&format!("available instead: {}", alternatives.join(", ")));
    }
}

/// Check daemon ownership and per-slot process counts. Each tablet slot has its
/// own FIFO; duplicate encoders on one FIFO corrupt its frames.
async fn check_processes(r: &mut Report, cfg: &FileConfig) {
    let inventory = match processes::same_user_processes() {
        Ok(inventory) => inventory,
        Err(error) => {
            r.line(Level::Warn, "process inspection", &error.to_string());
            return;
        }
    };
    let pid_file = crate::get_pid_path();
    let daemons = uscreen_config::linux::daemon::from_processes(&inventory, Some(&pid_file));
    let tracked = report_daemon(r, &daemons, &pid_file);
    let fifos = (0..cfg.max_tablets).map(fifo_path_for).collect::<Vec<_>>();
    let helpers = fifos
        .iter()
        .flat_map(|fifo| capture_pids(&inventory, CaptureRole::Helper, fifo))
        .collect::<Vec<_>>();
    report_helpers(r, &helpers, tracked, cfg.max_tablets);
    for fifo in fifos {
        report_encoders(r, &encoders_for_fifo(&inventory, &fifo), tracked, &fifo);
    }
}

fn report_daemon(r: &mut Report, daemons: &[u32], pid_file: &Path) -> Option<u32> {
    let tracked = daemons.first().copied();
    match tracked {
        Some(pid) => r.line(Level::Ok, "daemon", &format!("running, PID {pid}")),
        None => r.line(Level::Ok, "daemon", "not running"),
    }
    if daemons.len() > 1 {
        r.line(
            Level::Fail,
            "multiple uscreen daemons",
            &format!("{daemons:?}"),
        );
        r.hint("uscreen stop reaches validated same-user daemons even without a PID file; stop them before starting again");
    } else if tracked.is_none() && pid_file.exists() {
        r.line(
            Level::Warn,
            "PID file",
            "stale or not a daemon; no live daemon was found",
        );
        r.hint("uscreen start replaces the stale PID file");
    }
    tracked
}

fn capture_pids(inventory: &[Process], role: CaptureRole, fifo: &Path) -> Vec<u32> {
    let uid = unsafe { libc::getuid() };
    inventory
        .iter()
        .filter(|process| process.owned_by(uid) && process.capture_role(fifo) == Some(role))
        .map(|process| process.pid)
        .collect()
}

pub(crate) fn encoders_for_fifo(inventory: &[Process], fifo: &Path) -> Vec<u32> {
    capture_pids(inventory, CaptureRole::Encoder, fifo)
}

fn report_encoders(r: &mut Report, encoders: &[u32], tracked: Option<u32>, fifo: &Path) {
    if encoders.len() > 1 {
        r.line(
            Level::Fail,
            &format!("ffmpeg on {}", fifo.display()),
            &format!("{} running: {:?}", encoders.len(), encoders),
        );
        r.hint("two readers on one pipe corrupt frames; stop and start UScreen to retire matching capture processes before capture begins");
    } else if encoders.len() == 1 && tracked.is_none() {
        r.line(
            Level::Fail,
            &format!("ffmpeg on {}", fifo.display()),
            "orphaned",
        );
        r.hint("stop and start UScreen; capture startup retires processes matching this FIFO");
    } else {
        r.line(
            Level::Ok,
            &format!("ffmpeg on {}", fifo.display()),
            &format!("{}", encoders.len()),
        );
    }
}

fn report_helpers(r: &mut Report, helpers: &[u32], tracked: Option<u32>, max_tablets: u32) {
    if helpers.len() > max_tablets as usize {
        r.line(
            Level::Fail,
            "evdi_helper processes",
            &format!("{} running: {:?}", helpers.len(), helpers),
        );
        r.hint("more helpers than configured tablet slots: stop and start UScreen; capture startup retires matching helpers before attaching");
    } else if !helpers.is_empty() && tracked.is_none() {
        r.line(Level::Fail, "evdi_helper", "orphaned (no daemon owns it)");
        r.hint("start UScreen and reconnect the tablet; capture startup retires matching helpers before attaching");
    } else {
        r.line(
            Level::Ok,
            "evdi_helper processes",
            &format!("{}", helpers.len()),
        );
    }
}

async fn check_tablet(r: &mut Report, cfg: &FileConfig) -> Vec<String> {
    check_tablet_with(
        r,
        cfg,
        "adb",
        crate::runtime::load_sessions(&crate::runtime::runtime_dir().join("sessions.json")),
    )
    .await
}

async fn check_tablet_with(
    r: &mut Report,
    cfg: &FileConfig,
    adb: &str,
    sessions: Option<Vec<crate::runtime::TabletSession>>,
) -> Vec<String> {
    let Some(list) = output_of(adb, &["devices"]).await else {
        r.line(Level::Warn, "adb", "could not run");
        return Vec::new();
    };
    let mut devices: Vec<String> = list
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            (parts.next()? == "device").then(|| serial.to_owned())
        })
        .collect();
    devices.sort_by_key(|serial| transport_of(serial) != Transport::Usb);
    let sessions = diagnostic_sessions(r, cfg, adb, &devices, sessions).await;
    if sessions.is_empty() {
        if list.contains("unauthorized") {
            r.line(Level::Fail, "tablet", "attached but unauthorized");
            r.hint("accept the 'Allow USB debugging' prompt on the tablet");
        } else {
            r.line(Level::Warn, "tablet", "no active tablet session");
        }
    }
    let mut selected = Vec::new();
    for session in sessions {
        r.line(
            Level::Ok,
            "tablet session",
            &format!(
                "slot {}: {} (video {}, input {})",
                session.instance + 1,
                session.serial,
                session.video_port,
                session.input_port
            ),
        );
        if !devices.contains(&session.serial) {
            r.line(
                Level::Warn,
                "tablet",
                "active transport no longer reachable; waiting for daemon refresh",
            );
            continue;
        }
        report_transport(r, &session.serial);
        check_tablet_session(r, cfg, adb, &session).await;
        selected.push(session.serial);
    }
    selected
}

async fn diagnostic_sessions(
    r: &mut Report,
    cfg: &FileConfig,
    adb: &str,
    devices: &[String],
    sessions: Option<Vec<crate::runtime::TabletSession>>,
) -> Vec<crate::runtime::TabletSession> {
    match sessions {
        Some(sessions) => sessions,
        None => {
            r.line(
                Level::Warn,
                "tablet selection",
                "no live daemon session report; checking a candidate",
            );
            let mut identities = std::collections::HashMap::new();
            let devices = crate::unique_devices(devices, None, &mut identities, adb).await;
            crate::pick_device_with(&devices, None, adb)
                .await
                .into_iter()
                .map(|serial| crate::runtime::TabletSession {
                    serial,
                    instance: 0,
                    video_port: cfg.video_port,
                    input_port: cfg.input_port,
                })
                .collect()
        }
    }
}

async fn check_tablet_session(
    r: &mut Report,
    cfg: &FileConfig,
    adb: &str,
    session: &crate::runtime::TabletSession,
) {
    let cfg = &FileConfig {
        video_port: session.video_port,
        input_port: session.input_port,
        ..cfg.clone()
    };
    match output_of(adb, &["-s", &session.serial, "reverse", "--list"]).await {
        Some(reverse) => report_forwarding(r, cfg, &session.serial, &reverse),
        None => r.line(Level::Warn, "adb reverse", "could not query"),
    }
    // dumpsys media.player lists active playback, not decoder capabilities.
    // The installed app queries MediaCodecList behind its shell-only receiver.
    let codec_output = tokio::process::Command::new(adb)
        .args([
            "-s",
            &session.serial,
            "shell",
            "am",
            "broadcast",
            "--include-stopped-packages",
            "-n",
            "com.uscreen/.CodecReportReceiver",
            "-a",
            "com.uscreen.DECODER_CAPABILITIES",
        ])
        .output_bounded()
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    report_codec(r, cfg, codec_output.as_deref());

    let mut pm_args: Vec<&str> = vec!["-s", &session.serial];
    pm_args.extend_from_slice(&["shell", "pm", "list", "packages", "com.uscreen"]);
    if let Some(out) = output_of(adb, &pm_args).await {
        if out.contains("com.uscreen") {
            r.line(Level::Ok, "tablet app", "installed");
        } else {
            r.line(Level::Fail, "tablet app", "com.uscreen not installed");
            r.hint(&format!(
                "download uscreen.apk from {} then: adb -s {} install -r uscreen.apk",
                crate::update::RELEASES_PAGE,
                session.serial
            ));
        }
    }
}

fn report_codec(r: &mut Report, cfg: &FileConfig, out: Option<&str>) {
    let report = codec_inventory_text(out);
    let entries: Vec<_> = report.unwrap_or("").split(',').collect();
    let valid = entries.iter().all(|entry| {
        matches!(
            *entry,
            "hw8" | "hw10" | "sw8" | "sw10" | "unknown8" | "unknown10"
        )
    });
    let hevc = cfg.encoder.contains("hevc");
    if report == Some("none") {
        r.line(
            if hevc { Level::Fail } else { Level::Ok },
            "tablet codec",
            "no HEVC decoder reported by MediaCodecList",
        );
    } else if !valid {
        r.line(
            Level::Warn,
            "tablet codec",
            "HEVC capability unknown (no valid decoder inventory)",
        );
        r.hint("install the current tablet APK, then rerun doctor; host encoder selection does not prove tablet support");
    } else if hevc && cfg.ten_bit && !entries.iter().any(|entry| entry.ends_with("10")) {
        r.line(
            Level::Fail,
            "tablet codec",
            "HEVC Main10 not reported; 10-bit host stream is unsupported by this inventory",
        );
    } else {
        report_usable_decoders(r, cfg, &entries);
    }
}

fn codec_inventory_text(out: Option<&str>) -> Option<&str> {
    out.and_then(|text| text.split_once("USCREEN_CODECS_V1:").map(|(_, tail)| tail))
        .map(|tail| tail.split(['"', '\r', '\n']).next().unwrap_or("").trim())
}

fn decoder_matches_stream(entry: &str, cfg: &FileConfig) -> bool {
    !cfg.encoder.contains("hevc") || !cfg.ten_bit || entry.ends_with("10")
}

fn report_usable_decoders(r: &mut Report, cfg: &FileConfig, entries: &[&str]) {
    let usable: Vec<_> = entries
        .iter()
        .copied()
        .filter(|entry| decoder_matches_stream(entry, cfg))
        .collect();
    if usable.iter().any(|entry| entry.starts_with("hw")) {
        let detail = if usable.contains(&"hw10") {
            "hardware HEVC Main10 reported by Android"
        } else {
            "hardware HEVC reported by Android (8-bit)"
        };
        r.line(Level::Ok, "tablet codec", detail);
    } else if usable.iter().any(|entry| entry.starts_with("unknown")) {
        r.line(
            Level::Warn,
            "tablet codec",
            "HEVC decoder reported; acceleration unknown",
        );
    } else {
        r.line(
            Level::Warn,
            "tablet codec",
            "HEVC software decoder only; real-time performance is not guaranteed",
        );
    }
}

fn report_forwarding(r: &mut Report, cfg: &FileConfig, serial: &str, reverse: &str) {
    for (app, host) in [
        (crate::APP_VIDEO_PORT, cfg.video_port),
        (crate::APP_INPUT_PORT, cfg.input_port),
    ] {
        let remote = format!("tcp:{app}");
        let local = format!("tcp:{host}");
        let forwarded = reverse.lines().any(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            fields
                .windows(2)
                .any(|pair| pair == [remote.as_str(), local.as_str()])
        });
        if forwarded {
            r.line(
                Level::Ok,
                &format!("adb reverse {app}"),
                &format!("forwarded to {host}"),
            );
        } else {
            r.line(
                Level::Warn,
                &format!("adb reverse {app}"),
                &format!("not forwarded to {host}"),
            );
            r.hint(&format!("adb -s {serial} reverse {remote} {local}"));
        }
    }
}

async fn check_virtual_display(r: &mut Report, cfg: &FileConfig) {
    let connectors = vdisplay::evdi_connectors();
    if connectors.is_empty() {
        r.line(Level::Fail, "EVDI connector", "none found in sysfs");
        r.hint("the evdi module is loaded but exposes no DRM connector — reboot or re-add");
        return;
    }
    report_connectors(r, &connectors);

    let names: Vec<&str> = connectors.iter().map(|c| c.name.as_str()).collect();
    let Ok(response) = crate::kscreen::fetch().await else {
        return;
    };
    // Preserve doctor's text-output decoding; mapping consumes strict bytes.
    let text = String::from_utf8_lossy(&response.stdout);
    let parsed = crate::kscreen::parse(text.as_bytes());
    if matches!(parsed, Err(crate::kscreen::ParseError::InvalidJson)) {
        r.line(Level::Warn, "kscreen-doctor", "unparseable JSON output");
    }
    let Ok(outputs) = parsed else {
        return;
    };

    report_display_outputs(r, cfg, &names, &outputs);
}

fn report_display_outputs(
    r: &mut Report,
    cfg: &FileConfig,
    names: &[&str],
    outputs: &[crate::kscreen::Output],
) {
    for out in outputs {
        let name = out.label();
        if !names.contains(&name) {
            continue;
        }
        if !out.enabled {
            r.line(Level::Warn, "KDE output", &format!("{} is disabled", name));
            r.hint("the daemon enables it while an attached tablet uses it as a screen; nothing is rendered while it is off");
            continue;
        }
        let (w, h) = out.pixel_size;
        report_output_mode(r, cfg, name, w, h);
    }
}

fn report_connectors(r: &mut Report, connectors: &[vdisplay::EvdiConnector]) {
    for c in connectors {
        r.line(
            if c.connected { Level::Ok } else { Level::Warn },
            "EVDI connector",
            &format!(
                "{} ({})",
                c.name,
                if c.connected {
                    "connected"
                } else {
                    "disconnected — helper not running"
                }
            ),
        );
    }
}

fn report_output_mode(r: &mut Report, cfg: &FileConfig, name: &str, w: i64, h: i64) {
    // Auto resolution follows the tablet. Capture always negotiates the actual mode.
    if cfg.auto_resolution || (w as u32 == cfg.width && h as u32 == cfg.height) {
        r.line(
            Level::Ok,
            "KDE output mode",
            &format!("{} at {}x{}", name, w, h),
        );
    } else {
        r.line(
            Level::Warn,
            "KDE output mode",
            &format!(
                "{} is {}x{}, config says {}x{}",
                name, w, h, cfg.width, cfg.height
            ),
        );
        r.hint("capture uses this actual mode; choose the configured mode in Display Settings if desired");
    }
}

/// The on-screen keyboard pops up whenever a touchscreen is used, and the
/// tablet's touch device is exactly that as far as the desktop is concerned.
/// Reported because it is a global desktop setting, not something the daemon
/// should quietly decide on the user's behalf.
async fn check_osk(r: &mut Report, cfg: &FileConfig) {
    if !cfg.input_touch && !cfg.input_pen {
        r.line(
            Level::Ok,
            "input mapping",
            "disabled — no virtual input devices requested",
        );
        return;
    }
    match crate::desktop::Desktop::current() {
        crate::desktop::Desktop::X11 => report_x11_mapping_tools(r),
        crate::desktop::Desktop::KdeWayland => check_kwin_input(r).await,
        crate::desktop::Desktop::Other => {
            r.line(
                Level::Warn,
                "input mapping",
                "manual compositor-specific mapping may be required",
            );
            r.hint("KWin automation applies to KDE Wayland; use your compositor's input/output settings on other desktops. Support depends on that compositor.");
        }
    }
}

fn report_x11_mapping_tools(r: &mut Report) {
    for tool in ["xinput", "xrandr"] {
        if command_exists(tool) {
            r.line(Level::Ok, tool, "available for X11 input mapping");
        } else {
            r.line(Level::Fail, tool, "not installed");
            r.hint(&format!("install {tool} for automatic X11 input mapping"));
        }
    }
}

async fn check_kwin_input(r: &mut Report) {
    // Whether we can reach KWin at all decides whether touch and pen land on
    // the tablet's screen, so it is reported first and in its own right.
    match crate::kwin::backend().await {
        Some(b) => r.line(
            Level::Ok,
            "KWin D-Bus",
            &format!("reachable via {}", b.name()),
        ),
        None => {
            r.line(
                Level::Fail,
                "KWin D-Bus",
                "unreachable — touch and pen will drive the wrong screen",
            );
            r.hint(
                "the daemon maps the tablet's input onto the virtual display over KWin's \
                 D-Bus interface, using busctl (systemd) or qdbus. Install systemd's busctl, \
                 or a qdbus package (qdbus-qt6 on Debian/Ubuntu, qt6-tools elsewhere).",
            );
            return;
        }
    }

    // The live value, not the one in kwinrc: KWin does not re-read that file,
    // so the two disagree routinely and only this one reflects what happens.
    let Some(raw) =
        crate::kwin::get_property("/VirtualKeyboard", "org.kde.kwin.VirtualKeyboard", "mode").await
    else {
        return;
    };
    let mode: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    match mode.trim() {
        "1" | "2" => {
            r.line(Level::Warn, "on-screen keyboard", "pops up on touch input");
            r.hint("the daemon suppresses this while its touch devices exist, then restores the saved setting");
        }
        "0" => r.line(Level::Ok, "on-screen keyboard", "only when asked for"),
        _ => {}
    }
}

/// Whether plugging the cable in is actually enough on its own.
async fn check_autostart(r: &mut Report) {
    let enabled = output_of("systemctl", &["--user", "is-enabled", "uscreen.service"]).await;
    let load_state = output_of(
        "systemctl",
        &[
            "--user",
            "show",
            "uscreen.service",
            "-p",
            "LoadState",
            "--value",
        ],
    )
    .await
    .unwrap_or_default();
    report_autostart(r, load_state.trim(), enabled);
}

fn report_autostart(r: &mut Report, load_state: &str, enabled: Option<String>) {
    if load_state == "not-found" {
        r.line(
            Level::Warn,
            "start with the desktop",
            "service not installed",
        );
        r.hint("reinstall the UScreen package or run scripts/install.sh from the release tarball to install uscreen.service");
        return;
    }
    match enabled {
        Some(v) if v.trim() == "enabled" => {
            r.line(Level::Ok, "start with the desktop", "enabled");
        }
        Some(_) => {
            r.line(
                Level::Warn,
                "start with the desktop",
                "disabled — the daemon must be started by hand",
            );
            r.hint("systemctl --user enable --now uscreen.service");
        }
        None => {}
    }
}

async fn tablet_setting(
    program: &str,
    serial: Option<&str>,
    namespace: &str,
    key: &str,
) -> Option<String> {
    let serial = serial?;
    output_of(
        program,
        &["-s", serial, "shell", "settings", "get", namespace, key],
    )
    .await
}

/// Colour accuracy, for using the tablet to judge images rather than just to
/// hold windows. Everything here is a setting rather than a bug, but each one
/// silently ruins colour and none of them is visible from the host side.
async fn check_colour(r: &mut Report, serial: Option<&str>) {
    check_blue_light_filter(r, serial).await;
    check_tablet_colour_mode(r, serial).await;
    check_tablet_refresh_rate(r, serial).await;
    check_desktop_colour_profiles(r).await;
}

async fn check_blue_light_filter(r: &mut Report, serial: Option<&str>) {
    // Eye comfort / blue light filter warms the whole panel. Nothing on the
    // host can compensate, and it is easy to leave on by accident.
    match tablet_setting("adb", serial, "system", "blue_light_filter").await {
        Some(v) if v.trim() == "1" => {
            r.line(Level::Fail, "blue light filter", "ON — colours are warmed");
            r.hint("tablet: Settings → Display → Eye comfort shield → off");
        }
        Some(v) if v.trim() == "0" => r.line(Level::Ok, "blue light filter", "off"),
        _ => {}
    }
}

async fn check_tablet_colour_mode(r: &mut Report, serial: Option<&str>) {
    // Samsung's "Vivid" screen mode stretches saturation past sRGB. "Natural"
    // is the colour-accurate one.
    if let Some(v) = tablet_setting("adb", serial, "system", "screen_mode_setting").await {
        let v = v.trim().to_string();
        if v == "2" {
            r.line(Level::Ok, "tablet screen mode", "Natural (sRGB)");
        } else if !v.is_empty() && v != "null" {
            r.line(
                Level::Warn,
                "tablet screen mode",
                &format!("{} — not the sRGB-accurate mode", v),
            );
            r.hint("tablet: Settings → Display → Screen mode → Natural");
        }
    }
}

async fn check_tablet_refresh_rate(r: &mut Report, serial: Option<&str>) {
    // Samsung's "Motion smoothness" setting. On "Standard" the panel only
    // offers apps its 60 Hz modes, so the app's request for the fastest one
    // gets 60 and every frame waits an average of 8 ms for vsync instead of
    // 4. Seen on a Tab S9 Ultra: the app asked for the best mode and was
    // handed 60 Hz until this was switched to Adaptive.
    if let Some(v) = tablet_setting("adb", serial, "secure", "refresh_rate_mode").await {
        let v = v.trim().to_string();
        if v == "0" {
            r.line(
                Level::Warn,
                "tablet refresh rate",
                "Motion smoothness is Standard — the panel is held at 60 Hz",
            );
            r.hint("tablet: Settings → Display → Motion smoothness → Adaptive (120 Hz)");
        } else if !v.is_empty() && v != "null" {
            r.line(
                Level::Ok,
                "tablet refresh rate",
                "Motion smoothness: adaptive",
            );
        }
    }
}

async fn check_desktop_colour_profiles(r: &mut Report) {
    // KWin can colour-manage the virtual output, but only once a profile is
    // attached to it.
    let names: Vec<String> = vdisplay::evdi_connectors()
        .into_iter()
        .map(|c| c.name)
        .collect();
    let Some(outputs) = crate::kscreen::outputs().await else {
        return;
    };
    for out in outputs {
        let name = out.label();
        if !names.iter().any(|n| n == name) {
            continue;
        }
        let icc = &out.icc_profile;
        if icc.is_empty() {
            r.line(
                Level::Warn,
                "colour profile",
                "none assigned to the virtual display",
            );
            r.hint(
                "System Settings → Display → pick the UScreen display → Color Profile. \
                 A generic sRGB profile is the right baseline; a measured one needs a \
                 colorimeter pointed at the tablet.",
            );
        } else {
            r.line(Level::Ok, "colour profile", icc);
        }
    }
}

async fn check_version(r: &mut Report, cfg: &FileConfig) {
    if !cfg.check_updates {
        r.line(
            Level::Ok,
            "version",
            &format!(
                "{} (update check disabled)",
                crate::update::current_version()
            ),
        );
        return;
    }
    // One-shot: doctor is its own process and cannot read the daemon's daily
    // result, and ten seconds on the network is fine for a diagnostic.
    let cur = crate::update::current_version();
    match crate::update::latest_release_tag().await {
        Some(tag) => {
            if crate::update::is_newer(&tag, cur) {
                let latest = tag.trim().strip_prefix('v').unwrap_or(tag.trim());
                r.line(
                    Level::Warn,
                    "version",
                    &format!("{} — {} is available", cur, latest),
                );
                r.hint(crate::update::RELEASES_PAGE);
            } else {
                r.line(Level::Ok, "version", &format!("{} (latest)", cur));
            }
        }
        None => r.line(
            Level::Ok,
            "version",
            &format!("{} (could not check for updates)", cur),
        ),
    }
}

fn read_config_report(r: &mut Report, path: &Path) -> Option<FileConfig> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            r.line(
                Level::Ok,
                "config file",
                &format!("{} absent — using defaults", path.display()),
            );
            return None;
        }
        Err(error) => {
            r.line(
                Level::Fail,
                "config file",
                &format!("{} unreadable: {error}", path.display()),
            );
            r.hint("restore access to this file; runtime uses defaults until it can be read");
            return None;
        }
    };
    match toml::from_str(&text) {
        Ok(config) => {
            r.line(Level::Ok, "config file", &path.display().to_string());
            Some(config)
        }
        Err(error) => {
            r.line(
                Level::Fail,
                "config file",
                &format!("{} invalid: {error}", path.display()),
            );
            r.hint("repair the TOML; runtime uses defaults and partial updates preserve the invalid file");
            None
        }
    }
}

fn check_config(r: &mut Report, cfg: &FileConfig) {
    let path = config::config_path();
    // `cfg` has already been clamped, so compare against the raw file too: a
    // stale 200 Mbps on disk is worth reporting even though the daemon would
    // no longer act on it.
    let on_disk = read_config_report(r, &path);
    check_tablet_capacity(r, cfg);
    report_configured_display(r, cfg);
    report_configured_input(r, cfg);
    report_input_dependencies(r, cfg);
    report_configured_bitrate(r, cfg, on_disk.as_ref());
    report_configured_fps(r, cfg, on_disk.as_ref());
}

fn check_tablet_capacity(r: &mut Report, cfg: &FileConfig) {
    if cfg.max_tablets > 1 {
        let cards = crate::vdisplay::evdi_cards().len() as u32;
        if cards >= cfg.max_tablets {
            r.line(
                Level::Ok,
                "tablet slots",
                &format!("{} (EVDI devices: {})", cfg.max_tablets, cards),
            );
        } else {
            r.line(
                Level::Warn,
                "tablet slots",
                &format!(
                    "{} wanted, but only {} EVDI device(s) exist",
                    cfg.max_tablets, cards
                ),
            );
            r.hint(&format!(
                "for this boot: echo 1 | sudo tee /sys/devices/evdi/add   (repeat {} time(s)); \
                 for every boot: initial_device_count={} in /etc/modprobe.d/uscreen-evdi.conf",
                cfg.max_tablets - cards,
                cfg.max_tablets
            ));
        }
    }
}

fn report_configured_display(r: &mut Report, cfg: &FileConfig) {
    r.line(
        Level::Ok,
        "screen position",
        match cfg.position.as_str() {
            "left" => "left of the other screens",
            "above" => "above the other screens",
            "below" => "below the other screens",
            _ => "right of the other screens",
        },
    );
    r.line(
        Level::Ok,
        "mode",
        if cfg.pen_only {
            "graphics tablet — no display is streamed (switchable from the tablet)"
        } else {
            "second screen (switchable from the tablet)"
        },
    );
    r.line(
        Level::Ok,
        "resolution",
        &format!(
            "{}x{}{}",
            cfg.width,
            cfg.height,
            if cfg.auto_resolution {
                " (auto, follows the tablet)"
            } else {
                " (fixed)"
            }
        ),
    );
}

fn report_configured_input(r: &mut Report, cfg: &FileConfig) {
    let mut on: Vec<&str> = Vec::new();
    if cfg.input_touch {
        on.push("touch");
    }
    if cfg.input_pen {
        on.push("pen");
    }
    if cfg.input_pen && cfg.input_pointer {
        on.push("pointer");
    }
    if on.is_empty() {
        // A deliberate choice, not a fault: opting out is what the switches
        // are for.
        r.line(
            Level::Ok,
            "input devices",
            "none — the tablet is display-only (touch and pen are ignored)",
        );
    } else {
        r.line(
            Level::Ok,
            "input devices",
            &format!("{} (created while a tablet is attached)", on.join(", ")),
        );
    }
}

fn report_input_dependencies(r: &mut Report, cfg: &FileConfig) {
    if cfg.pen_only && !cfg.input_pen {
        r.line(
            Level::Warn,
            "input devices",
            "graphics-tablet mode needs the pen device — the daemon will start as a second screen instead",
        );
        r.hint("enable the pen in the settings panel, or set input_pen = true in config.toml");
    }
    if cfg.input_pointer && !cfg.input_pen {
        r.hint("input_pointer only takes effect together with input_pen");
    }
}

fn report_configured_bitrate(r: &mut Report, cfg: &FileConfig, on_disk: Option<&FileConfig>) {
    let raw_bitrate = on_disk.map(|c| c.bitrate).unwrap_or(cfg.bitrate);
    if !(MIN_BITRATE_KBPS..=MAX_BITRATE_KBPS).contains(&raw_bitrate) {
        r.line(
            Level::Warn,
            "bitrate on disk",
            &format!(
                "{} Mbps — clamped to {} Mbps at runtime",
                raw_bitrate as f64 / 1000.0,
                cfg.bitrate as f64 / 1000.0
            ),
        );
        r.hint("rewrite it via uscreen-gui (or the tablet settings) to make the file agree");
    } else {
        r.line(
            Level::Ok,
            "bitrate",
            &format!("{} Mbps", cfg.bitrate as f64 / 1000.0),
        );
    }
}

fn report_configured_fps(r: &mut Report, cfg: &FileConfig, on_disk: Option<&FileConfig>) {
    let raw_fps = on_disk.map(|c| c.fps).unwrap_or(cfg.fps);
    if !(MIN_FPS..=MAX_FPS).contains(&raw_fps) {
        r.line(
            Level::Warn,
            "fps on disk",
            &format!("{} — clamped to {} at runtime", raw_fps, cfg.fps),
        );
        if raw_fps > MAX_FPS {
            r.hint("the generated EDID is capped at 90 Hz, so anything above is duplicate frames");
        } else {
            r.hint(&format!(
                "set fps to at least {MIN_FPS} in the settings panel or config.toml"
            ));
        }
    } else {
        r.line(Level::Ok, "fps", &format!("{}", cfg.fps));
    }
}

pub async fn run() -> Result<()> {
    println!("=== uscreen doctor ===");

    let mut r = Report::new();
    // Loaded once: every load logs a warning when it clamps a stale value, and
    // repeating that between report sections buries the report itself.
    let cfg = FileConfig::load();

    section("Kernel modules");
    check_modules(&mut r, &cfg);

    section("Tools");
    check_tools(&mut r, &cfg).await;

    section("Processes");
    check_processes(&mut r, &cfg).await;

    section("Tablet");
    let serials = check_tablet(&mut r, &cfg).await;

    section("Virtual display");
    check_virtual_display(&mut r, &cfg).await;

    section("Autostart");
    check_autostart(&mut r).await;

    section("Desktop");
    check_osk(&mut r, &cfg).await;

    section("Colour");
    for serial in &serials {
        check_colour(&mut r, Some(serial)).await;
    }

    section("Configuration");
    check_version(&mut r, &cfg).await;
    check_config(&mut r, &cfg);

    println!();
    if r.failures > 0 {
        println!(
            "{} problem(s) and {} warning(s) found — fix the FAIL lines first.",
            r.failures, r.warnings
        );
    } else if r.warnings > 0 {
        println!("No blocking problems, {} warning(s).", r.warnings);
    } else {
        println!("Everything checks out.");
    }
    Ok(())
}

/// Use the same serial-form policy as connection preference and Wi-Fi setup.
/// Network ADB does not by itself prove the tablet is using a Wi-Fi radio.
fn report_transport(r: &mut Report, serial: &str) {
    if transport_of(serial) == Transport::Network {
        r.line(Level::Warn, "transport", Transport::Network.label());
        r.hint("plug in USB; the daemon prefers it when both transports identify the same tablet");
    } else {
        r.line(Level::Ok, "transport", "USB");
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn t234_input_diagnostics_follow_desktop_and_requested_devices() {
        const TEST: &str =
            "doctor::tests::t234_input_diagnostics_follow_desktop_and_requested_devices";
        if let Ok(input) = std::env::var("USCREEN_T234_INPUT") {
            let enabled = input == "true";
            let cfg = super::FileConfig {
                input_touch: enabled,
                input_pen: enabled,
                ..Default::default()
            };
            let mut report = super::Report::new();
            super::check_osk(&mut report, &cfg).await;
            let expected: u32 = std::env::var("USCREEN_T234_FAILURES")
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(
                report.failures,
                expected,
                "T234: {:?}",
                report.messages.borrow()
            );
            let trace = std::path::PathBuf::from(std::env::var_os("USCREEN_T234_TRACE").unwrap());
            assert_eq!(
                trace.exists(),
                expected > 0,
                "T234: queried KWin without a KDE input requirement"
            );
            if enabled && crate::desktop::Desktop::current() == crate::desktop::Desktop::Other {
                assert!(report.warnings > 0);
                assert!(report
                    .messages
                    .borrow()
                    .iter()
                    .any(|message| message.contains("manual")));
            }
            if !enabled {
                assert!(report
                    .messages
                    .borrow()
                    .iter()
                    .any(|message| message.contains("disabled")));
            }
            return;
        }
        for (desktop, session, inputs, failures) in [
            ("GNOME", "wayland", true, 0),
            ("sway", "wayland", true, 0),
            ("KDE", "wayland", false, 0),
            ("X-Cinnamon", "x11", true, 0),
            ("KDE", "wayland", true, 1),
            ("", "wayland", true, 0),
        ] {
            run_t234_fixture(TEST, desktop, session, inputs, failures);
        }
    }

    fn run_t234_fixture(test: &str, desktop: &str, session: &str, inputs: bool, failures: u32) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        for tool in ["busctl", "qdbus", "qdbus6", "qdbus-qt6", "qdbus-qt5"] {
            let path = dir.path().join(tool);
            std::fs::write(
                &path,
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$USCREEN_T234_TRACE\"\nexit 1\n",
            )
            .unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        for tool in ["xinput", "xrandr"] {
            let path = dir.path().join(tool);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env("PATH", dir.path())
            .env("XDG_CURRENT_DESKTOP", desktop)
            .env("XDG_SESSION_TYPE", session)
            .env("USCREEN_T234_INPUT", inputs.to_string())
            .env("USCREEN_T234_FAILURES", failures.to_string())
            .env("USCREEN_T234_TRACE", dir.path().join("trace"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn t372_diagnostics_read_raw_dimensions_from_the_shared_inventory() {
        let outputs =
            crate::kscreen::parse(include_bytes!("../../testdata/kscreen-inventory.json")).unwrap();
        let mut report = super::Report::new();
        super::report_display_outputs(
            &mut report,
            &super::FileConfig::default(),
            &["DVI-I-1", "HDMI-1"],
            &outputs,
        );
        let messages = report.messages.borrow().join("\n");
        assert!(messages.contains("1280"), "{messages}");
        assert!(messages.contains("800"), "{messages}");
        assert!(messages.contains("HDMI-1 is disabled"), "{messages}");
        assert_eq!(report.failures, 0);
    }

    mod lookup_fixture {
        include!("../../testdata/executable_lookup.rs");
    }

    #[test]
    fn t232_executable_discovery_without_which() {
        lookup_fixture::check(
            super::command_exists,
            "doctor::tests::t232_executable_discovery_without_which",
        );
    }

    #[cfg(feature = "inproc-encoder")]
    #[test]
    fn t284_doctor_rejects_vaapi_even_when_ffmpeg_lists_it() {
        use super::*;
        let inventory = " V....D h264_vaapi H.264/AVC\n V....D hevc_vaapi HEVC\n";
        for encoder in ["h264_vaapi", "hevc_vaapi", "vaapih264enc"] {
            let cfg = FileConfig {
                encoder: encoder.into(),
                ..Default::default()
            };
            let mut report = Report::new();
            report_encoder_availability(&mut report, &cfg, inventory);
            assert_eq!(report.failures, 1, "T284: incompatible encoder advertised");
            assert!(report
                .messages
                .borrow()
                .join("\n")
                .contains("without --features inproc-encoder"));
        }
    }

    mod daemon_fixture {
        include!("../../testdata/daemon_process.rs");
    }

    #[tokio::test]
    async fn t251_doctor_recovers_daemons_without_broad_orphan_matches() {
        if std::env::var_os("USCREEN_T251_CHILD").is_some() {
            check_t251_report(true).await;
            return;
        }
        let fixture = daemon_fixture::Fixture::new();
        let daemon = fixture.start(&[]);
        let diagnostic = fixture.start(&["doctor"]);
        let unrelated = fixture.start_named(
            "evdi_helper",
            &["--capture-fifo", "/unrelated/capture.fifo"],
        );
        let fifo = fixture.root.path().join("runtime/uscreen/capture.fifo");
        let helper =
            fixture.start_named("evdi_helper", &["--capture-fifo", fifo.to_str().unwrap()]);
        let encoder = fixture.start_named("ffmpeg", &["-i", fifo.to_str().unwrap()]);
        run_t251_report_child(
            &fixture,
            diagnostic.pid(),
            Some(daemon.pid()),
            "doctor::tests::t251_doctor_recovers_daemons_without_broad_orphan_matches",
        );
        for pid in [
            daemon.pid(),
            diagnostic.pid(),
            unrelated.pid(),
            helper.pid(),
            encoder.pid(),
        ] {
            assert!(
                uscreen_config::linux::processes::Process::read(pid).is_some(),
                "T251: diagnostics signalled a fixture"
            );
        }
    }

    #[tokio::test]
    async fn t251_doctor_ignores_diagnostic_commands_and_unrelated_helpers() {
        if std::env::var_os("USCREEN_T251_CHILD").is_some() {
            check_t251_report(false).await;
            return;
        }
        let fixture = daemon_fixture::Fixture::new();
        let doctor = fixture.start(&["doctor"]);
        let status = fixture.start(&["status"]);
        let unrelated = fixture.start_named(
            "evdi_helper",
            &["--capture-fifo", "/unrelated/capture.fifo"],
        );
        run_t251_report_child(
            &fixture,
            doctor.pid(),
            None,
            "doctor::tests::t251_doctor_ignores_diagnostic_commands_and_unrelated_helpers",
        );
        for pid in [doctor.pid(), status.pid(), unrelated.pid()] {
            assert!(uscreen_config::linux::processes::Process::read(pid).is_some());
        }
    }

    #[test]
    fn t251_orphan_remediation_uses_validated_lifecycle_commands() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let mut report = Report::new();
        report_helpers(&mut report, &[11], None, 1);
        report_helpers(&mut report, &[11, 12], Some(10), 1);
        report_encoders(&mut report, &[13], None, &root.path().join("capture.fifo"));
        report_encoders(
            &mut report,
            &[13, 14],
            Some(10),
            &root.path().join("capture.fifo"),
        );
        report_daemon(&mut report, &[10, 20], &root.path().join("pid"));
        assert_eq!(report.failures, 5);
        let text = report.messages.borrow().join("\n");
        assert!(!text.contains("pkill"), "T251: {text}");
        assert!(text.contains("uscreen stop reaches validated same-user daemons"));
    }

    #[test]
    fn t251_inventory_filters_foreign_owners_before_classification() {
        use super::*;
        let fixture = daemon_fixture::Fixture::new();
        let daemon = fixture.start(&["start"]);
        let fifo = fixture.root.path().join("capture.fifo");
        let helper =
            fixture.start_named("evdi_helper", &["--capture-fifo", fifo.to_str().unwrap()]);
        let mut daemon_process = Process::read(daemon.pid()).unwrap();
        let mut helper_process = Process::read(helper.pid()).unwrap();
        assert_eq!(
            uscreen_config::linux::daemon::from_processes(&[daemon_process.clone()], None),
            [daemon.pid()]
        );
        assert_eq!(
            capture_pids(&[helper_process.clone()], CaptureRole::Helper, &fifo),
            [helper.pid()]
        );
        daemon_process.uid = daemon_process.uid.wrapping_add(1);
        helper_process.uid = helper_process.uid.wrapping_add(1);
        assert!(uscreen_config::linux::daemon::from_processes(&[daemon_process], None).is_empty());
        assert!(capture_pids(&[helper_process], CaptureRole::Helper, &fifo).is_empty());
    }

    fn run_t251_report_child(
        fixture: &daemon_fixture::Fixture,
        diagnostic: u32,
        daemon: Option<u32>,
        name: &str,
    ) {
        let runtime = fixture.root.path().join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("USCREEN_T251_CHILD", "1")
            .env("USCREEN_T251_DIAGNOSTIC", diagnostic.to_string())
            .env("USCREEN_T251_DAEMON", daemon.unwrap_or(0).to_string())
            .env("HOME", fixture.root.path())
            .env("XDG_RUNTIME_DIR", runtime)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T251: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn check_t251_report(running: bool) {
        use super::*;
        let diagnostic = std::env::var("USCREEN_T251_DIAGNOSTIC").unwrap();
        let daemon = std::env::var("USCREEN_T251_DAEMON").unwrap();
        let path = crate::get_pid_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for stale in [
            None,
            Some("invalid"),
            Some("4294967295"),
            Some(diagnostic.as_str()),
            Some(daemon.as_str()),
        ] {
            let _ = std::fs::remove_file(&path);
            if let Some(text) = stale {
                std::fs::write(&path, text).unwrap();
            }
            let mut report = Report::new();
            check_processes(
                &mut report,
                &FileConfig {
                    max_tablets: 1,
                    ..Default::default()
                },
            )
            .await;
            let text = report.messages.borrow().join("\n");
            assert_eq!(report.failures, 0, "T251: {stale:?}: {text}");
            assert!(
                !text.contains("pkill"),
                "T251: broad process remediation: {text}"
            );
            let state = if running {
                format!("running, PID {daemon}")
            } else {
                "not running".into()
            };
            assert!(text.contains(&format!("daemon: {state}")), "T251: {text}");
            let helpers = usize::from(running);
            assert!(
                text.contains(&format!("evdi_helper processes: {helpers}")),
                "T251: {text}"
            );
        }
    }

    #[test]
    fn t278_doctor_uses_the_same_mdns_network_classification() {
        use super::*;
        for serial in [
            "adb-TABLET-nonce._adb-tls-connect._tcp",
            "tablet._adb._tcp.local.",
            "192.0.2.1:5555",
        ] {
            let mut report = Report::new();
            report_transport(&mut report, serial);
            assert_eq!(report.warnings, 1, "T278: {serial}");
            let text = report.messages.borrow().join("\n");
            assert!(text.contains("Network ADB"), "T278: {text}");
        }
        let mut usb = Report::new();
        report_transport(&mut usb, "USB_TABLET");
        assert_eq!(usb.warnings, 0);
        assert!(usb
            .messages
            .borrow()
            .iter()
            .any(|line| line == "transport: USB"));
    }

    #[test]
    fn t313_doctor_accepts_the_cli_vaapi_alias() {
        use super::*;
        let inventory = " V....D h264_vaapi H.264/AVC (VAAPI)\n V....D libx264 H.264/AVC\n";
        for (encoder, failures) in [("h264_vaapi", 0), ("vaapih264enc", 0), ("hevc_vaapi", 1)] {
            let mut cfg = FileConfig {
                encoder: encoder.into(),
                ..Default::default()
            };
            cfg.sanitize();
            assert_eq!(cfg.encoder, encoder, "T313: accepted configuration changed");
            let mut report = Report::new();
            report_encoder_availability(&mut report, &cfg, inventory);
            let expected = if cfg!(feature = "inproc-encoder") {
                1
            } else {
                failures
            };
            assert_eq!(report.failures, expected, "T313: encoder {encoder}");
            assert_eq!(report.warnings, 0);
            if cfg!(feature = "inproc-encoder") {
                assert!(report
                    .messages
                    .borrow()
                    .join("\n")
                    .contains("VAAPI is unavailable in this in-process build"));
            } else if failures > 0 {
                assert!(report
                    .messages
                    .borrow()
                    .iter()
                    .any(|message| message == "available instead: h264_vaapi, libx264"));
            }
        }
    }

    #[test]
    fn t301_saved_bitrate_warnings_cover_both_limits() {
        use super::*;
        for (bitrate, expected, warnings) in [
            (0, "0 Mbps — clamped to 1 Mbps at runtime", 1),
            (999, "0.999 Mbps — clamped to 1 Mbps at runtime", 1),
            (1000, "1 Mbps", 0),
            (60000, "60 Mbps", 0),
            (60001, "60.001 Mbps — clamped to 60 Mbps at runtime", 1),
        ] {
            let raw = FileConfig {
                bitrate,
                ..Default::default()
            };
            let mut effective = raw.clone();
            effective.sanitize();
            let mut report = Report::new();
            report_configured_bitrate(&mut report, &effective, Some(&raw));
            assert_eq!(report.warnings, warnings, "T301: saved bitrate {bitrate}");
            assert_eq!(report.failures, 0);
            assert!(
                report
                    .messages
                    .borrow()
                    .iter()
                    .any(|message| message.contains(expected)),
                "T301: inaccurate bitrate diagnostic: {:?}",
                report.messages.borrow()
            );
        }
    }

    #[test]
    fn t301_saved_fps_warnings_cover_both_limits() {
        use super::*;
        for (fps, warnings) in [(0, 1), (9, 1), (10, 0), (90, 0), (91, 1)] {
            let raw = FileConfig {
                fps,
                ..Default::default()
            };
            let mut effective = raw.clone();
            effective.sanitize();
            let mut report = Report::new();
            report_configured_fps(&mut report, &effective, Some(&raw));
            assert_eq!(report.warnings, warnings, "T301: saved fps {fps}");
            assert_eq!(report.failures, 0);
            if warnings > 0 {
                let text = report.messages.borrow().join("\n");
                assert!(text.contains(&format!("{fps} — clamped to {} at runtime", effective.fps)));
                assert_eq!(
                    text.contains("above is duplicate frames"),
                    fps > 90,
                    "T301: high-rate guidance must not be given for a low rate"
                );
            }
        }
    }

    #[test]
    fn t149_config_health_distinguishes_invalid_missing_and_valid_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        for content in ["fps = invalid\n", "fps = 'wrong type'\n"] {
            std::fs::write(&path, content).unwrap();
            let mut report = super::Report::new();
            assert!(super::read_config_report(&mut report, &path).is_none());
            assert_eq!(report.failures, 1, "malformed config was reported healthy");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let mut report = super::Report::new();
        assert!(super::read_config_report(&mut report, &path).is_none());
        assert_eq!(report.failures, 1);
        std::fs::remove_dir(&path).unwrap();
        let mut report = super::Report::new();
        assert!(super::read_config_report(&mut report, &path).is_none());
        assert_eq!(report.failures, 0);
        assert!(report
            .messages
            .borrow()
            .iter()
            .any(|line| line.contains("defaults")));
        std::fs::write(&path, "fps = 120\n").unwrap();
        let mut report = super::Report::new();
        assert_eq!(
            super::read_config_report(&mut report, &path).unwrap().fps,
            120,
            "diagnostics must retain raw values before runtime clamps"
        );
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn t098_codec_report_requires_decoder_evidence() {
        let cfg = FileConfig {
            encoder: "hevc_nvenc".into(),
            ..Default::default()
        };
        for (raw, ten_bit, warnings, failures, message) in [
            (None, false, 1, 0, "unknown"),
            (Some("video/hevc Main10"), false, 1, 0, "unknown"),
            (
                Some("data=\"USCREEN_CODECS_V1:none\""),
                false,
                0,
                1,
                "no HEVC decoder",
            ),
            (
                Some("data=\"USCREEN_CODECS_V1:sw10\""),
                false,
                1,
                0,
                "software",
            ),
            (
                Some("data=\"USCREEN_CODECS_V1:hw8\""),
                true,
                0,
                1,
                "Main10 not reported",
            ),
            (
                Some("data=\"USCREEN_CODECS_V1:hw10\""),
                true,
                0,
                0,
                "hardware HEVC Main10",
            ),
            (
                Some("data=\"USCREEN_CODECS_V1:unknown10\""),
                false,
                1,
                0,
                "acceleration unknown",
            ),
        ] {
            let mut report = Report::new();
            report_codec(
                &mut report,
                &FileConfig {
                    ten_bit,
                    ..cfg.clone()
                },
                raw,
            );
            assert_eq!(
                (report.warnings, report.failures),
                (warnings, failures),
                "{raw:?}"
            );
            let text = report.messages.borrow().join("\n");
            assert!(text.contains(message), "{text}");
            assert!(
                !text.contains("switch the encoder") && !text.contains("tick 10-bit"),
                "{text}"
            );
        }
    }

    #[tokio::test]
    async fn t099_doctor_targets_active_sessions_and_actual_ports() {
        let temp = tempfile::tempdir().unwrap();
        let adb = temp.path().join("adb");
        let log = temp.path().join("calls");
        std::fs::write(&adb, format!(r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
case "$*" in
  devices) printf 'List of devices attached\nPHONE\tdevice\nTABLET\tdevice\n192.0.2.1:5555\tdevice\nEXTRA\tdevice\n';;
  '-s 192.0.2.1:5555 reverse --list') printf 'USB tcp:8890 tcp:19000\nUSB tcp:8891 tcp:19100\n';;
  '-s EXTRA reverse --list') printf 'USB tcp:8890 tcp:19004\nUSB tcp:8891 tcp:19104\n';;
  *'pm path com.uscreen') [ "$2" = TABLET ] && echo package:/app/uscreen.apk;;
  *'pm list packages com.uscreen') echo package:com.uscreen;;
esac
exit 0
"#, log.display())).unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut report = Report::new();
        let sessions = vec![
            crate::runtime::TabletSession {
                serial: "192.0.2.1:5555".into(),
                instance: 0,
                video_port: 19000,
                input_port: 19100,
            },
            crate::runtime::TabletSession {
                serial: "EXTRA".into(),
                instance: 2,
                video_port: 19004,
                input_port: 19104,
            },
        ];
        let selected = check_tablet_with(
            &mut report,
            &FileConfig::default(),
            adb.to_str().unwrap(),
            Some(sessions),
        )
        .await;
        assert_eq!(selected, ["192.0.2.1:5555", "EXTRA"]);
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(!calls.contains("-s PHONE"), "{calls}");
        let messages = report.messages.borrow().join("\n");
        assert!(!messages.contains("not forwarded"), "{messages}");
        assert!(
            messages.contains("19004") && messages.contains("19104"),
            "{messages}"
        );
        std::fs::write(&log, "").unwrap();
        let mut report = Report::new();
        let selected = check_tablet_with(
            &mut report,
            &FileConfig::default(),
            adb.to_str().unwrap(),
            None,
        )
        .await;
        assert_eq!(selected, ["TABLET"]);
        assert!(report.messages.borrow().join("\n").contains("candidate"));
    }

    #[test]
    fn t076_forwarding_checks_exact_app_to_host_ports_and_hints_selected_tablet() {
        let cfg = FileConfig {
            video_port: 19000,
            input_port: 19001,
            ..Default::default()
        };
        let mut r = Report::new();
        report_forwarding(
            &mut r,
            &cfg,
            "TABLET",
            "USB tcp:19000 tcp:19000\nUSB tcp:19001 tcp:19001\n",
        );
        assert_eq!(r.warnings, 2);
        let hints = r.messages.borrow().join("\n");
        assert!(hints.contains("adb -s TABLET reverse tcp:8890 tcp:19000"));
        assert!(hints.contains("adb -s TABLET reverse tcp:8891 tcp:19001"));
        let mut healthy = Report::new();
        report_forwarding(
            &mut healthy,
            &cfg,
            "TABLET",
            "USB tcp:8890 tcp:19000\nUSB tcp:8891 tcp:19001\n",
        );
        assert_eq!(healthy.warnings, 0);
    }

    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn t023_missing_or_unloadable_helper_is_reported() {
        let mut r = Report::new();
        check_tools_with_helper(
            &mut r,
            &FileConfig::default(),
            Path::new("/nonexistent-uscreen-test/evdi_helper"),
        )
        .await;
        assert!(r
            .messages
            .borrow()
            .iter()
            .any(|m| m.contains("evdi_helper") && m.contains("could not execute")));
    }

    #[test]
    fn t024_one_helper_per_tablet_is_healthy() {
        let mut r = Report::new();
        report_helpers(&mut r, &[11, 12], Some(10), 2);
        assert_eq!(r.failures, 0);
        report_helpers(&mut r, &[11, 12, 13], Some(10), 2);
        assert_eq!(r.failures, 1);
    }

    #[test]
    fn t025_auto_resolution_does_not_report_a_working_mode_as_corrupt() {
        let mut r = Report::new();
        let cfg = FileConfig {
            auto_resolution: true,
            ..Default::default()
        };
        report_output_mode(&mut r, &cfg, "DVI-I-1", 1920, 1080);
        assert_eq!(r.failures, 0);
        assert!(!r.messages.borrow().iter().any(|m| m.contains("skewed")));
    }

    #[tokio::test]
    async fn t026_colour_queries_target_the_selected_tablet() {
        let path = std::env::temp_dir().join(format!("uscreen-adb-colour-{}", std::process::id()));
        std::fs::write(
            &path,
            "#!/bin/sh\n[ \"$1 $2\" = '-s TABLET' ] || exit 1\nprintf '%s' \"$7\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        for (namespace, key) in [
            ("system", "blue_light_filter"),
            ("system", "screen_mode_setting"),
            ("secure", "refresh_rate_mode"),
        ] {
            assert_eq!(
                tablet_setting(path.to_str().unwrap(), Some("TABLET"), namespace, key)
                    .await
                    .as_deref(),
                Some(key)
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn t058_packaged_udev_hint_works_outside_a_source_checkout() {
        let root = std::env::temp_dir().join(format!("uscreen-udev-doctor-{}", std::process::id()));
        std::fs::create_dir_all(root.join("usr/lib/udev/rules.d")).unwrap();
        std::fs::write(
            root.join("usr/lib/udev/rules.d/60-uscreen-uinput.rules"),
            "rule",
        )
        .unwrap();
        let hint = uinput_hint(&root);
        assert!(!hint.contains("packaging/"));
        assert!(hint.contains("udevadm trigger"));
        assert!(hint.contains("seat"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn t060_missing_service_is_distinguished_from_disabled_service() {
        let mut r = Report::new();
        report_autostart(&mut r, "not-found", Some(String::new()));
        let messages = r.messages.borrow().join("\n");
        assert!(messages.contains("not installed"), "{messages}");
        assert!(!messages.contains("enable --now"));
    }
}
