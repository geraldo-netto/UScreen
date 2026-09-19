// Linux supervision keeps its existing crate-root modules and test identities.
#[cfg(target_os = "linux")]
include!("linux_main.rs");

#[cfg(windows)]
mod windows_main;
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    windows_main::run()
}

#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("UScreen currently builds for Linux and the Windows diagnostics preview");
