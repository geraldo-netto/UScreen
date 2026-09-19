//! Linux dynamic status and assigned-tablet probes.
use super::{Capabilities, Source};
use crate::{apply_tablet_sessions, autostart_enabled, command_exists, pid_path, Status};
use std::process::Command;
use uscreen_config::{commands::SyncCommandExt, linux::daemon, runtime};
#[derive(Default)]
pub(super) struct Platform {
    daemon: daemon::StatusProbe,
}

impl Source for Platform {
    fn capabilities(&mut self) -> Capabilities {
        Capabilities {
            daemon_binary: crate::find_uscreen_bin().is_some(),
            ffmpeg: command_exists("ffmpeg"),
            adb: command_exists("adb"),
            autostart: autostart_enabled(),
        }
    }

    fn dynamic(&mut self, capabilities: &Capabilities) -> Status {
        let pid = self.daemon.poll(Some(&pid_path()));
        let mut status = Status {
            daemon_binary: capabilities.daemon_binary,
            ffmpeg_ok: capabilities.ffmpeg,
            adb_ok: capabilities.adb,
            autostart: capabilities.autostart,
            daemon_running: pid.is_some(),
            daemon_pid: pid.unwrap_or_default(),
            evdi_count: std::fs::read_to_string("/sys/devices/evdi/count")
                .ok()
                .and_then(|text| text.trim().parse().ok())
                .unwrap_or(-1),
            uinput_ok: std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/uinput")
                .is_ok(),
            ..Status::default()
        };
        let sessions = runtime::runtime_dir()
            .ok()
            .and_then(|dir| runtime::load_sessions(&dir.join("sessions.json")))
            .unwrap_or_default();
        status.pipe_ceiling = uscreen_config::linux::pipe::ceiling_bytes();
        status.pipe_capacities = sessions
            .iter()
            .map(|session| {
                let capacity = runtime::fifo_path_for(session.instance)
                    .ok()
                    .and_then(|path| uscreen_config::linux::pipe::effective_bytes(&path));
                (session.instance, capacity)
            })
            .collect();
        query_tablets(&mut status, sessions, capabilities.adb, || {
            Command::new("adb")
                .args(["devices", "-l"])
                .output_bounded()
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        });
        status
    }
}

pub(super) fn query_tablets(
    status: &mut Status,
    sessions: Vec<runtime::TabletSession>,
    adb_available: bool,
    devices: impl FnOnce() -> std::io::Result<String>,
) {
    if !adb_available || sessions.is_empty() {
        return;
    }
    if let Ok(text) = devices() {
        apply_tablet_sessions(status, &text, sessions);
    }
}
