//! Discovery-only status: no runtime state is created or backend implied.
use super::{Capabilities, Source};
use crate::Status;
#[derive(Default)]
pub(super) struct Platform {}
impl Source for Platform {
    fn capabilities(&mut self) -> Capabilities {
        let diagnostics = std::sync::Arc::new(uscreen_config::diagnostics::collect());
        Capabilities {
            daemon_binary: crate::find_uscreen_bin().is_some(),
            ffmpeg: diagnostics.tools[1].verified(),
            adb: diagnostics.tools[0].verified(),
            autostart: crate::autostart_enabled(),
            diagnostics: Some(diagnostics),
        }
    }
    fn dynamic(&mut self, capabilities: &Capabilities) -> Status {
        Status {
            daemon_binary: capabilities.daemon_binary,
            ffmpeg_ok: capabilities.ffmpeg,
            adb_ok: capabilities.adb,
            autostart: capabilities.autostart,
            diagnostics: capabilities.diagnostics.clone(),
            ..Default::default()
        }
    }
}
