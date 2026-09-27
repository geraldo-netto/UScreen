//! T531 native interactive contracts; run as an ordinary Windows user.
use super::*;
use crate::tray_state::{QUIT, SETTINGS};
use windows_sys::Win32::{System::Threading::*, UI::Shell::*};
fn wait(condition: impl Fn() -> bool) {
    blent_config::lifecycle::wait_until(std::time::Duration::from_secs(10), || Ok(condition()))
        .unwrap();
}
fn title(hwnd: usize) -> String {
    let mut value = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd as HWND, value.as_mut_ptr(), value.len() as i32) };
    String::from_utf16_lossy(&value[..len as usize])
}
fn send(hwnd: usize, message: u32, w: usize, l: isize) {
    unsafe {
        SendMessageW(hwnd as HWND, message, w, l);
    }
}
fn fixture(root: &std::path::Path) -> PathBuf {
    let source = root.join("gui.rs");
    std::fs::write(&source, r#"fn main() { std::fs::write(std::env::current_exe().unwrap().with_file_name("opened"), b"settings").unwrap(); }"#).unwrap();
    let exe = root.join("GUI café & spaces.exe");
    assert!(std::process::Command::new("rustc")
        .args(["--crate-name", "tray_gui", "--out-dir"])
        .arg(root)
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .status()
        .unwrap()
        .success());
    exe
}
fn counters() -> (u32, u32) {
    unsafe {
        (
            GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS),
            GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS),
        )
    }
}
#[tokio::test]
async fn t531_native_actions_latest_status_shell_restart_and_resource_retirement() {
    let root = tempfile::tempdir().unwrap();
    let exe = fixture(root.path());
    let (_, stopping) = watch::channel(State::Stopping);
    assert!(Tray::start(stopping, watch::channel(false).0).is_err());
    let (state, receiver) = watch::channel(State::Starting);
    let (stop, stopped) = watch::channel(false);
    let tray = Tray::start_at(receiver, stop, exe).unwrap();
    let hwnd = tray.target.0.lock().unwrap().unwrap();
    assert!(title(hwnd).contains("Starting Blent"));
    state.send_replace(State::Prepared(4));
    state.send_replace(State::Waiting);
    // Native callback reads the producer's latest snapshot even before the async wakeup.
    unsafe {
        PostMessageW(hwnd as HWND, window::REFRESH, 0, 0);
    }
    wait(|| title(hwnd).contains("No tablet connected"));
    assert!(title(hwnd).contains("Display and input unavailable"));
    for id in [0, 3, 65537, usize::MAX] {
        send(hwnd, WM_COMMAND, id, 0);
    }
    send(hwnd, window::EVENT, 0, NIN_SELECT as isize); // wrong icon identity
    assert!(!root.path().join("opened").exists());
    send(hwnd, window::EVENT, 0, (1 << 16) | NIN_SELECT as isize);
    wait(|| root.path().join("opened").exists());
    std::fs::remove_file(root.path().join("opened")).unwrap();
    state.send_replace(State::Stopping);
    send(hwnd, WM_COMMAND, SETTINGS, 0);
    send(hwnd, WM_COMMAND, QUIT, 0);
    assert!(!*stopped.borrow());
    assert!(!root.path().join("opened").exists());
    state.send_replace(State::Prepared(1));
    cancel_menu(hwnd);
    restart(hwnd);
    send(hwnd, WM_COMMAND, QUIT, 0);
    assert!(*stopped.borrow());
    drop(tray);
    assert_eq!(unsafe { IsWindow(hwnd as HWND) }, 0);
    let data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd as HWND,
        uID: 1,
        ..Default::default()
    };
    assert_eq!(unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }, 0);
    cleanup_cycles().await;
}
fn restart(hwnd: usize) {
    let data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd as HWND,
        uID: 1,
        ..Default::default()
    };
    assert_ne!(unsafe { Shell_NotifyIconW(NIM_DELETE, &data) }, 0);
    // T531: an update racing shell loss must leave the message loop alive.
    unsafe {
        SendMessageW(hwnd as HWND, window::REFRESH, 0, 0);
    }
    let message = unsafe { RegisterWindowMessageW(resources::wide("TaskbarCreated").as_ptr()) };
    unsafe {
        SendMessageW(hwnd as HWND, message, 0, 0);
    }
    wait(|| unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } != 0);
    wait(|| title(hwnd).contains("USB prepared: 1"));
}
async fn cleanup_cycles() {
    let before = counters();
    for _ in 0..5 {
        let (state, receiver) = watch::channel(State::Waiting);
        let (stop, _) = watch::channel(false);
        let tray = Tray::start(receiver, stop).unwrap();
        let hwnd = tray.target.0.lock().unwrap().unwrap();
        state.send_replace(State::Unavailable);
        tokio::task::yield_now().await;
        wait(|| title(hwnd).contains("USB connection unavailable"));
        drop(state);
        tokio::task::yield_now().await;
        wait(|| unsafe { IsWindow(hwnd as HWND) } == 0);
        drop(tray);
    }
    assert_eq!(counters(), before, "T531: leaked native tray resources");
}
#[test]
fn t531_native_menu_bounds_failure_and_settings_failure() {
    for state in [State::Waiting, State::Stopping] {
        let menu = resources::Menu::new(state).unwrap();
        assert_eq!(unsafe { GetMenuItemCount(menu.0) }, 5);
        for id in [SETTINGS, QUIT] {
            let flags = unsafe { GetMenuState(menu.0, id as u32, MF_BYCOMMAND) };
            assert_eq!(flags & MF_GRAYED != 0, state == State::Stopping);
        }
    }
    assert!(resources::check(false).is_err());
    assert!(resources::check(true).is_ok());
    let (stop, _) = watch::channel(false);
    assert!(NativeActions {
        stop,
        settings: PathBuf::from("Z:\\blent-t531-missing\\gui.exe")
    }
    .settings()
    .is_err());
    let notification = resources::Notification::new(std::ptr::null_mut()).unwrap();
    assert!(notification.update(State::Waiting, false).is_err());
    for state in [State::Prepared(0), State::Prepared(255), State::Pen] {
        assert_eq!(notification.data(state).szTip[127], 0);
    }
}

fn cancel_menu(hwnd: usize) {
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd as HWND, std::ptr::null_mut()) };
    let cancel = std::thread::spawn(move || {
        wait(|| {
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            unsafe {
                GetGUIThreadInfo(thread_id, &mut info);
            }
            info.flags & GUI_INMENUMODE != 0
        });
        unsafe {
            PostMessageW(hwnd as HWND, WM_CANCELMODE, 0, 0);
        }
    });
    send(hwnd, window::EVENT, 0, (1 << 16) | WM_CONTEXTMENU as isize);
    cancel.join().unwrap();
}
