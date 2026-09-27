//! Platform-independent USB assignment status; no native capture/input claim.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TabletSession {
    pub serial: String,
    pub instance: u32,
    pub video_port: u16,
    pub input_port: u16,
}
