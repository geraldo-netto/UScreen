//! T631: retain the Linux native camera probe without importing it on Windows.
#[cfg(target_os = "linux")]
#[path = "camera-feedback-probe/linux.rs"]
mod linux;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("camera-feedback-probe requires the Linux camera backend");
    std::process::exit(2);
}
