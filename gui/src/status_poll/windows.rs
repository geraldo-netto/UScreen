//! Read the daemon-owned USB assignments; display/input remain unsupported.
use super::{Capabilities, Source};
use crate::Status;
#[derive(Default)]
pub(super) struct Platform {}
impl Source for Platform {
    fn capabilities(&mut self) -> Capabilities {
        let diagnostics = std::sync::Arc::new(blent_config::diagnostics::collect());
        Capabilities {
            daemon_binary: crate::find_blent_bin().is_some(),
            ffmpeg: diagnostics.tools[1].verified(),
            adb: diagnostics.tools[0].verified(),
            autostart: crate::autostart_enabled(),
            diagnostics: Some(diagnostics),
        }
    }
    fn dynamic(&mut self, capabilities: &Capabilities) -> Status {
        let path = crate::platform::runtime_path().ok();
        dynamic_at(path.as_deref(), capabilities)
    }
}
fn dynamic_at(path: Option<&std::path::Path>, capabilities: &Capabilities) -> Status {
    let sessions = path
        .and_then(|path| blent_config::windows::lifecycle::load_sessions(path))
        .unwrap_or_default();
    let owner = path
        .and_then(|path| blent_config::windows::lifecycle::status(path).ok())
        .flatten();
    Status {
        tablet_connected: owner.is_some() && !sessions.is_empty(),
        tablet_model: sessions
            .iter()
            .map(|session| session.serial.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        daemon_running: owner.is_some(),
        daemon_pid: owner.map(|identity| identity.pid).unwrap_or_default(),
        daemon_binary: capabilities.daemon_binary,
        ffmpeg_ok: capabilities.ffmpeg,
        adb_ok: capabilities.adb,
        autostart: capabilities.autostart,
        diagnostics: capabilities.diagnostics.clone(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t525_usb_status_uses_only_live_daemon_assignments() {
        let caps = Capabilities {
            daemon_binary: true,
            ffmpeg: false,
            adb: true,
            autostart: false,
            diagnostics: None,
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("runtime");
        assert!(!dynamic_at(None, &caps).daemon_running);
        assert!(!dynamic_at(Some(&path), &caps).tablet_connected);
        let session = blent_config::windows::lifecycle::Session::start(&path).unwrap();
        session
            .publish_sessions(&[blent_config::tablets::TabletSession {
                serial: "USB café".into(),
                instance: 0,
                video_port: 8890,
                input_port: 8891,
            }])
            .unwrap();
        let status = dynamic_at(Some(&path), &caps);
        assert!(status.tablet_connected);
        assert!(status.daemon_running);
        assert_eq!(status.tablet_model, "USB café");
        assert!(status.adb_ok);
        assert!(!status.ffmpeg_ok);
        assert!(!status.autostart);
        session.shutdown().unwrap();
        assert!(!dynamic_at(Some(&path), &caps).tablet_connected);
        // Real default runtime is observed only; never written by this test.
        let _ = Platform::default().dynamic(&caps);
    }
}
