//! Isolated native device adapter: absence of PipeWire never prevents display startup.
#[cfg(target_os = "linux")]
#[path = "audio/native.rs"]
mod native;
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    native::run()
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Native audio is unavailable on this operating system")
}
