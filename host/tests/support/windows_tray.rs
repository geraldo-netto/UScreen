//! T531: inspect only the isolated daemon's native notification window.
use windows_sys::Win32::{Foundation::*, UI::WindowsAndMessaging::*};
struct Search {
    pid: u32,
    hwnd: HWND,
}
unsafe extern "system" fn candidate(hwnd: HWND, context: LPARAM) -> i32 {
    let search = &mut *(context as *mut Search);
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let mut class = [0u16; 128];
    let len = GetClassNameW(hwnd, class.as_mut_ptr(), 128);
    if pid == search.pid
        && String::from_utf16_lossy(&class[..len as usize]).starts_with("BlentTray-")
    {
        search.hwnd = hwnd;
        return 0;
    }
    1
}
pub fn find(pid: u32) -> HWND {
    let mut search = Search {
        pid,
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(candidate), &mut search as *mut Search as LPARAM);
    }
    search.hwnd
}
pub fn title(pid: u32) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(find(pid), buffer.as_mut_ptr(), 256) };
    String::from_utf16_lossy(&buffer[..len as usize])
}
pub fn quit(pid: u32) {
    let hwnd = find(pid);
    assert!(!hwnd.is_null(), "T531 missing native tray");
    unsafe {
        SendMessageW(hwnd, WM_COMMAND, blent::tray_state::QUIT, 0);
    }
}

pub fn wait_status(pid: u32, expected: &str) {
    blent_config::lifecycle::wait_until(std::time::Duration::from_secs(10), || {
        Ok(title(pid).contains(expected))
    })
    .unwrap();
}
