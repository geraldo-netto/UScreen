//! Discovery-only status: no runtime state is created or backend implied.
use super::{Capabilities, Source};
use crate::{command_exists, Status};
#[derive(Default)]
pub(super) struct Platform {}
impl Source for Platform {
    fn capabilities(&mut self) -> Capabilities {
        Capabilities {
            daemon_binary: crate::find_uscreen_bin().is_some(),
            ffmpeg: command_exists("ffmpeg"),
            adb: command_exists("adb"),
            autostart: crate::autostart_enabled(),
        }
    }
    fn dynamic(&mut self, capabilities: &Capabilities) -> Status {
        Status {
            daemon_binary: capabilities.daemon_binary,
            ffmpeg_ok: capabilities.ffmpeg,
            adb_ok: capabilities.adb,
            autostart: capabilities.autostart,
            ..Default::default()
        }
    }
}
