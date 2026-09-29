use std::{
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    UI::WindowsAndMessaging::*,
};
pub static DOWN: AtomicUsize = AtomicUsize::new(0);
pub static UP: AtomicUsize = AtomicUsize::new(0);
pub static TOUCH: AtomicUsize = AtomicUsize::new(0);
pub static TOUCH_UP: AtomicUsize = AtomicUsize::new(0);
unsafe extern "system" fn window_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            DOWN.fetch_add(1, Ordering::SeqCst);
            0
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            UP.fetch_add(1, Ordering::SeqCst);
            0
        }
        WM_POINTERUP => {
            TOUCH_UP.fetch_add(1, Ordering::SeqCst);
            TOUCH.fetch_add(1, Ordering::SeqCst);
            DefWindowProcW(window, message, w, l)
        }
        WM_POINTERDOWN | WM_POINTERUPDATE => {
            TOUCH.fetch_add(1, Ordering::SeqCst);
            DefWindowProcW(window, message, w, l)
        }
        _ => DefWindowProcW(window, message, w, l),
    }
}
pub struct Window {
    handle: HWND,
    class: Vec<u16>,
    cursor: POINT,
}
impl Window {
    pub fn new(x: i32, y: i32) -> Self {
        let class: Vec<_> = format!("BlentInputTest-{}", std::process::id())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassW(&window_class) }, 0);
        let mut cursor = POINT::default();
        assert_ne!(unsafe { GetCursorPos(&mut cursor) }, 0);
        let handle = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                x - 100,
                y - 100,
                200,
                200,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!handle.is_null());
        Self {
            handle,
            class,
            cursor,
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.handle);
            UnregisterClassW(self.class.as_ptr(), ptr::null_mut());
            SetCursorPos(self.cursor.x, self.cursor.y);
        }
    }
}
pub fn pump() {
    let end = Instant::now() + Duration::from_millis(40);
    while Instant::now() < end {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
