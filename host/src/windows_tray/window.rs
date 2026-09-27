//! Message-loop boundary. Copyright (c) 2026 Geraldo Netto.
use super::{resources::*, NativeActions, Target};
use crate::tray_state::{State, SETTINGS};
use anyhow::Result;
use std::sync::{mpsc::SyncSender, Mutex, OnceLock};
use tokio::sync::watch;
use windows_sys::Win32::{
    Foundation::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Shell::*, WindowsAndMessaging::*},
};
pub(super) const EVENT: u32 = WM_APP + 1;
pub(super) const REFRESH: u32 = WM_APP + 2;
struct Core {
    state: Mutex<watch::Receiver<State>>,
    actions: NativeActions,
    shell_restart: u32,
    target: Target,
    notification: OnceLock<Notification>,
}
impl Core {
    fn handle(&self, hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
        if message == REFRESH || message == self.shell_restart {
            self.refresh(message == self.shell_restart);
            return true;
        }
        match message {
            WM_COMMAND => self.dispatch(wparam),
            EVENT => {
                if let Err(error) = self.event(hwnd, lparam) {
                    eprintln!("{error:#}");
                }
            }
            _ => return false,
        }
        true
    }
    fn refresh(&self, add: bool) {
        if let Some(notification) = self.notification.get() {
            if let Err(error) = notification.update(self.state(), add) {
                eprintln!("{error:#}");
            }
        }
    }
    fn state(&self) -> State {
        *self.state.lock().unwrap().borrow()
    }
    fn dispatch(&self, id: usize) {
        if let Err(error) = self.state().dispatch(id, &self.actions) {
            eprintln!("{error:#}");
        }
    }
    fn event(&self, hwnd: HWND, value: LPARAM) -> Result<()> {
        if (value as u32 >> 16) != 1 {
            return Ok(());
        }
        match value as u32 & 0xffff {
            WM_CONTEXTMENU => self.menu(hwnd)?,
            NIN_SELECT | 1025 => self.dispatch(SETTINGS),
            _ => {}
        }
        Ok(())
    }
    fn menu(&self, hwnd: HWND) -> Result<()> {
        let menu = Menu::new(self.state())?;
        let mut point = POINT::default();
        unsafe {
            GetCursorPos(&mut point);
            SetForegroundWindow(hwnd);
            let id = TrackPopupMenu(
                menu.0,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                hwnd,
                std::ptr::null(),
            );
            self.dispatch(id as usize);
            let data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                ..Default::default()
            };
            Shell_NotifyIconW(NIM_SETFOCUS, &data);
            PostMessageW(hwnd, WM_NULL, 0, 0);
        }
        Ok(())
    }
}
struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}
pub(super) fn run(
    state: watch::Receiver<State>,
    actions: NativeActions,
    ready: SyncSender<Result<Target>>,
) {
    let result = serve(state, actions, &ready);
    if let Err(error) = result {
        let _ = ready.send(Err(error));
    }
}
fn serve(
    state: watch::Receiver<State>,
    actions: NativeActions,
    ready: &SyncSender<Result<Target>>,
) -> Result<()> {
    let mut core = Core {
        notification: OnceLock::new(),
        target: Target::default(),
        state: Mutex::new(state),
        actions,
        shell_restart: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
    };
    anyhow::ensure!(
        core.state() != State::Stopping,
        "Tray cannot start during shutdown"
    );
    let class = Class::new()?;
    let window = Window(unsafe {
        CreateWindowExW(
            0,
            class.name.as_ptr(),
            wide("Blent").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            &core as *const Core as *const _,
        )
    });
    check(!window.0.is_null())?;
    let notification = Notification::new(window.0)?;
    notification.update(core.state(), true)?;
    *core.target.0.lock().unwrap() = Some(window.0 as usize);
    check(core.notification.set(notification).is_ok())?;
    let _ = ready.send(Ok(core.target.clone()));
    messages();
    core.notification.take();
    Ok(())
}
fn messages() {
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
pub(super) unsafe extern "system" fn callback(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        WM_NCDESTROY => {
            let core = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Core;
            if let Some(core) = core.as_ref() {
                *core.target.0.lock().unwrap() = None;
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        WM_DESTROY => {
            PostQuitMessage(0);
        }
        _ => {
            if route(hwnd, message, wparam, lparam) {
                return 0;
            }
        }
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}
unsafe fn route(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    let core = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Core;
    if let Some(core) = core.as_ref() {
        return core.handle(hwnd, message, wparam, lparam);
    }
    false
}
