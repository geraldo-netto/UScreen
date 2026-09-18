//! Dynamic status stays fresh; program/autostart probes have a bounded cache.
use crate::{apply_tablet_sessions, autostart_enabled, command_exists, pid_path, Status};
use std::process::Command;
use std::time::{Duration, Instant};
use uscreen_config::{commands::SyncCommandExt, linux::daemon, runtime};

const CAPABILITY_TTL: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct Capabilities {
    ffmpeg: bool,
    adb: bool,
    autostart: bool,
}

trait Source {
    fn capabilities(&mut self) -> Capabilities;
    fn dynamic(&mut self, capabilities: &Capabilities) -> Status;
}

#[derive(Default)]
struct Platform {
    daemon: daemon::StatusProbe,
}

impl Source for Platform {
    fn capabilities(&mut self) -> Capabilities {
        Capabilities {
            ffmpeg: command_exists("ffmpeg"),
            adb: command_exists("adb"),
            autostart: autostart_enabled(),
        }
    }

    fn dynamic(&mut self, capabilities: &Capabilities) -> Status {
        let pid = self.daemon.poll(Some(&pid_path()));
        let mut status = Status {
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

fn query_tablets(
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

struct Sampler<S> {
    source: S,
    cached: Option<(Instant, Capabilities)>,
}

impl<S: Source> Sampler<S> {
    fn poll(&mut self, now: Instant, force: bool) -> Status {
        let stale = self
            .cached
            .as_ref()
            .is_none_or(|(when, _)| now.duration_since(*when) >= CAPABILITY_TTL);
        if force || stale {
            self.cached = Some((now, self.source.capabilities()));
        }
        self.source.dynamic(&self.cached.as_ref().unwrap().1)
    }
}

pub struct StatusPoller(Sampler<Platform>);
impl Default for StatusPoller {
    fn default() -> Self {
        Self(Sampler {
            source: Platform::default(),
            cached: None,
        })
    }
}
impl StatusPoller {
    pub fn poll(&mut self, force: bool) -> Status {
        self.0.poll(Instant::now(), force)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        static_calls: usize,
        dynamic_calls: usize,
        installed: bool,
    }
    impl Source for Fake {
        fn capabilities(&mut self) -> Capabilities {
            self.static_calls += 1;
            Capabilities {
                ffmpeg: self.installed,
                adb: self.installed,
                autostart: self.installed,
            }
        }
        fn dynamic(&mut self, caps: &Capabilities) -> Status {
            self.dynamic_calls += 1;
            Status {
                ffmpeg_ok: caps.ffmpeg,
                adb_ok: caps.adb,
                autostart: caps.autostart,
                evdi_count: self.dynamic_calls as i32,
                ..Status::default()
            }
        }
    }
    #[test]
    fn t409_capability_ttl_reduces_probes_and_preserves_dynamic_refresh() {
        let mut sampler = Sampler {
            source: Fake::default(),
            cached: None,
        };
        let start = Instant::now();
        for index in 0..30 {
            let status = sampler.poll(start + Duration::from_secs(index * 2), false);
            assert_eq!(status.evdi_count, index as i32 + 1);
        }
        assert_eq!(sampler.source.static_calls, 6);
        assert_eq!(sampler.source.dynamic_calls, 30);
    }
    #[test]
    fn t409_external_changes_expire_and_actions_force_refresh() {
        let mut sampler = Sampler {
            source: Fake::default(),
            cached: None,
        };
        let start = Instant::now();
        assert!(!sampler.poll(start, false).adb_ok);
        sampler.source.installed = true;
        assert!(!sampler.poll(start + Duration::from_secs(9), false).adb_ok);
        assert!(sampler.poll(start + Duration::from_secs(10), false).adb_ok);
        sampler.source.installed = false;
        assert!(!sampler.poll(start + Duration::from_secs(11), true).adb_ok);
        assert_eq!(sampler.source.static_calls, 3);
    }

    #[test]
    fn t409_no_idle_adb_calls_and_live_assignment_refresh() {
        let calls = std::cell::Cell::new(0);
        let devices = || {
            calls.set(calls.get() + 1);
            Ok("List of devices attached\nTABLET device model:Test_Tablet\nPHONE device model:Phone\n".into())
        };
        for _ in 0..30 {
            let mut status = Status::default();
            query_tablets(&mut status, vec![], true, devices);
            assert!(!status.tablet_connected);
        }
        assert_eq!(calls.get(), 0);
        let session = runtime::TabletSession {
            serial: "TABLET".into(),
            instance: 0,
            video_port: 19000,
            input_port: 20000,
        };
        let mut status = Status::default();
        query_tablets(&mut status, vec![session.clone()], false, devices);
        assert_eq!(calls.get(), 0);
        query_tablets(&mut status, vec![session.clone()], true, devices);
        assert!(status.tablet_connected);
        assert_eq!(status.tablet_model, "Test Tablet");
        assert_eq!(calls.get(), 1);
        let mut failed = Status::default();
        query_tablets(&mut failed, vec![session], true, || {
            Err(std::io::ErrorKind::TimedOut.into())
        });
        assert!(!failed.tablet_connected);
    }
}
