//! `uscreen doctor` — one-shot check of everything that has to be true for the
//! pipeline to work, with an actionable hint for every failure.
//!
//! This is deliberately the first thing to run whenever the stream "is laggy
//! again". The most common cause is not the pipeline at all but orphaned
//! `evdi_helper`/`ffmpeg` processes from a previous run: several writers on the
//! shared capture FIFO interleave at pipe granularity, and no amount of
//! restarting the daemon fixes it until they are killed.

use crate::capture::fifo_path_for;
use crate::config::{self, FileConfig, MAX_BITRATE_KBPS, MAX_FPS, MIN_BITRATE_KBPS, MIN_FPS};
use crate::vdisplay;
use anyhow::Result;
use std::path::Path;
use uscreen_config::commands::AsyncCommandExt;

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

/// PIDs whose executable name matches exactly (`pgrep -x`).
async fn pids_exact(name: &str) -> Vec<u32> {
    parse_pids(output_of("pgrep", &["-x", name]).await)
}

/// PIDs whose full command line matches a pattern (`pgrep -f`).
async fn pids_full(pattern: &str) -> Vec<u32> {
    parse_pids(output_of("pgrep", &["-f", pattern]).await)
}

fn parse_pids(out: Option<String>) -> Vec<u32> {
    out.unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
        .collect()
}

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
                     /etc/modprobe.d/uscreen-evdi.conf && sudo modprobe -r evdi && sudo modprobe evdi",
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
    let daemons = pids_exact("uscreen").await;
    let helpers = pids_exact("evdi_helper").await;

    let tracked = report_daemon(r);

    // `pgrep -x uscreen` also matches this very process — excluding it is not
    // cosmetic: reporting ourselves as an orphan would send the user off to
    // `pkill -x uscreen` chasing a process that never existed.
    let self_pid = std::process::id();
    let untracked: Vec<u32> = daemons
        .iter()
        .copied()
        .filter(|p| *p != self_pid && Some(*p) != tracked)
        .collect();
    if !untracked.is_empty() {
        r.line(
            Level::Fail,
            "orphaned uscreen daemons",
            &format!("{:?}", untracked),
        );
        r.hint("these fight over the same FIFO and ports — kill them: pkill -x uscreen");
    }

    report_helpers(r, &helpers, tracked, cfg.max_tablets);
    for instance in 0..cfg.max_tablets {
        let fifo = fifo_path_for(instance);
        let encoders = pids_full(&format!("ffmpeg.*{}([[:space:]]|$)", fifo)).await;
        report_encoders(r, &encoders, tracked, &fifo);
    }
}

fn report_daemon(r: &mut Report) -> Option<u32> {
    // The PID file is the daemon's single slot; anything running beside it is
    // untracked and `uscreen stop` will never reach it.
    let pid_file = crate::get_pid_path();
    let tracked: Option<u32> = std::fs::read_to_string(&pid_file)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .filter(|pid| config::daemon_is_running(*pid));

    match tracked {
        Some(pid) => r.line(Level::Ok, "daemon", &format!("running, PID {}", pid)),
        None if pid_file.exists() => {
            r.line(Level::Warn, "daemon", "stale PID file, not running");
            r.hint(&format!("rm {}", pid_file.display()));
        }
        None => r.line(Level::Ok, "daemon", "not running"),
    }

    tracked
}

fn report_encoders(r: &mut Report, encoders: &[u32], tracked: Option<u32>, fifo: &str) {
    if encoders.len() > 1 {
        r.line(
            Level::Fail,
            &format!("ffmpeg on {fifo}"),
            &format!("{} running: {:?}", encoders.len(), encoders),
        );
        r.hint("two readers on one pipe corrupt frames — kill the strays");
    } else if encoders.len() == 1 && tracked.is_none() {
        r.line(Level::Fail, &format!("ffmpeg on {fifo}"), "orphaned");
        r.hint(&format!("pkill -f 'ffmpeg.*{}'", fifo));
    } else {
        r.line(
            Level::Ok,
            &format!("ffmpeg on {fifo}"),
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
        r.hint("more helpers than configured tablet slots: stop UScreen, inspect these PIDs and remove strays before restarting");
    } else if !helpers.is_empty() && tracked.is_none() {
        r.line(Level::Fail, "evdi_helper", "orphaned (no daemon owns it)");
        r.hint("pkill -x evdi_helper");
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
    devices.sort_by_key(|serial| serial.contains(':'));
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
    let Some(json) = output_of("kscreen-doctor", &["-j"]).await else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
        r.line(Level::Warn, "kscreen-doctor", "unparseable JSON output");
        return;
    };
    let Some(outputs) = value.get("outputs").and_then(|o| o.as_array()) else {
        return;
    };

    report_display_outputs(r, cfg, &names, outputs);
}

fn report_display_outputs(
    r: &mut Report,
    cfg: &FileConfig,
    names: &[&str],
    outputs: &[serde_json::Value],
) {
    for out in outputs {
        let name = out.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if !names.contains(&name) {
            continue;
        }
        let enabled = out
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            r.line(Level::Warn, "KDE output", &format!("{} is disabled", name));
            r.hint("the daemon enables it while an attached tablet uses it as a screen; nothing is rendered while it is off");
            continue;
        }
        let w = out
            .pointer("/size/width")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let h = out
            .pointer("/size/height")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
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
async fn check_osk(r: &mut Report) {
    if std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("x11") {
        for tool in ["xinput", "xrandr"] {
            if command_exists(tool) {
                r.line(Level::Ok, tool, "available for X11 input mapping");
            } else {
                r.line(Level::Fail, tool, "not installed");
                r.hint(&format!("install {tool} for automatic X11 input mapping"));
            }
        }
        return;
    }
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
        let name = out.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if !names.iter().any(|n| n == name) {
            continue;
        }
        let icc = out
            .get("iccProfilePath")
            .and_then(|v| v.as_str())
            .unwrap_or("");
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
    check_osk(&mut r).await;

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

/// A network serial is `host:port`; a USB serial never contains a colon.
/// Worth reporting because the two transports differ by far more than the
/// median suggests — the Wi-Fi tail is several times worse.
fn report_transport(r: &mut Report, serial: &str) {
    if serial.contains(':') {
        r.line(
            Level::Warn,
            "transport",
            "Wi-Fi — expect occasional stutter",
        );
        r.hint("plug the USB cable in for steady latency; the daemon prefers it automatically");
    } else {
        r.line(Level::Ok, "transport", "USB");
    }
}

#[cfg(test)]
mod tests {
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
            assert_eq!(report.failures, failures, "T313: encoder {encoder}");
            assert_eq!(report.warnings, 0);
            if failures > 0 {
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
