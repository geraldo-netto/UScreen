//! Read-only daemon identity and dependency status; no tablet backend is implied.
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
        let owner = crate::platform::runtime_path()
            .ok()
            .and_then(|path| blent_config::windows::lifecycle::status(&path).ok())
            .flatten();
        Status {
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
}
