//! Native notification-area adapter. Copyright (c) 2026 Geraldo Netto.
use crate::tray_state::{Actions, State};
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};
use tokio::sync::watch;
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
mod resources;
#[cfg(test)]
mod tests;
mod window;

/// Owns exactly one shell icon and its message thread.
pub struct Tray {
    target: Target,
    updates: tokio::task::JoinHandle<()>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Tray {
    pub fn start(state: watch::Receiver<State>, stop: watch::Sender<bool>) -> Result<Self> {
        Self::start_with_updates(state, stop, watch::channel(None).1)
    }
    pub fn start_with_updates(
        state: watch::Receiver<State>,
        stop: watch::Sender<bool>,
        release: watch::Receiver<crate::update::Available>,
    ) -> Result<Self> {
        let launcher = blent_config::windows::paths::system()?.join("rundll32.exe");
        Self::start_at(state, stop, settings_program(), release, launcher)
    }
    fn start_at(
        state: watch::Receiver<State>,
        stop: watch::Sender<bool>,
        settings: PathBuf,
        release: watch::Receiver<crate::update::Available>,
        launcher: PathBuf,
    ) -> Result<Self> {
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let updates = state.clone();
        let releases = release.clone();
        let thread = thread::spawn(move || {
            window::run(
                state,
                release,
                NativeActions {
                    stop,
                    settings,
                    launcher,
                },
                ready,
            )
        });
        let result = started
            .recv()
            .context("Tray thread exited during startup")?;
        let target = match result {
            Ok(target) => target,
            Err(error) => {
                let _ = thread.join();
                return Err(error);
            }
        };
        let updates_target = target.clone();
        let updates = tokio::spawn(forward(updates, releases, updates_target));
        Ok(Self {
            target,
            updates,
            thread: Some(thread),
        })
    }
}
async fn forward(
    mut state: watch::Receiver<State>,
    mut release: watch::Receiver<crate::update::Available>,
    target: Target,
) {
    let mut release_open = true;
    loop {
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() { break; }
                target.post(window::REFRESH);
            }
            changed = release.changed(), if release_open => {
                release_open = changed.is_ok();
                target.post(window::REFRESH);
            }
        }
    }
    target.post(WM_CLOSE);
}
impl Drop for Tray {
    fn drop(&mut self) {
        self.updates.abort();
        self.target.post(WM_CANCELMODE);
        self.target.post(WM_CLOSE);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn settings_program() -> PathBuf {
    let sibling = std::env::current_exe()
        .unwrap_or_default()
        .with_file_name("blent-gui.exe");
    if sibling.is_file() {
        return sibling;
    }
    blent_config::windows::programs::find_in(
        "blent-gui",
        &std::env::var_os("PATH").unwrap_or_default(),
    )
    .unwrap_or(sibling)
}
struct NativeActions {
    stop: watch::Sender<bool>,
    settings: PathBuf,
    launcher: PathBuf,
}
impl Actions for NativeActions {
    fn settings(&self) -> Result<()> {
        blent_config::commands::spawn_reaped(&mut std::process::Command::new(&self.settings))
            .map(|_| ())
            .context("Open Blent Settings")
    }
    fn release(&self) -> Result<()> {
        blent_config::commands::spawn_reaped(
            std::process::Command::new(&self.launcher)
                .args(["url.dll,FileProtocolHandler", crate::update::RELEASES_PAGE]),
        )
        .map(|_| ())
        .context("Open Blent release page")
    }
    fn quit(&self) {
        self.stop.send_replace(true);
    }
}

#[derive(Clone, Default)]
struct Target(Arc<Mutex<Option<usize>>>);
impl Target {
    fn post(&self, message: u32) {
        if let Some(hwnd) = *self.0.lock().unwrap() {
            unsafe {
                PostMessageW(hwnd as HWND, message, 0, 0);
            }
        }
    }
}
