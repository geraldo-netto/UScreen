use super::*;
use crate::{
    direct_input::{Config, Event, Session},
    input_mapping::MonitorInventory,
    windows::monitors::NativeInventory,
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::*,
};
static DOWN: AtomicUsize = AtomicUsize::new(0);
static UP: AtomicUsize = AtomicUsize::new(0);
static TOUCH: AtomicUsize = AtomicUsize::new(0);
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
        WM_POINTERDOWN | WM_POINTERUP | WM_POINTERUPDATE => {
            TOUCH.fetch_add(1, Ordering::SeqCst);
            DefWindowProcW(window, message, w, l)
        }
        _ => DefWindowProcW(window, message, w, l),
    }
}
struct Window {
    handle: HWND,
    class: Vec<u16>,
    cursor: POINT,
}
impl Window {
    fn new(x: i32, y: i32) -> Self {
        let class: Vec<_> = format!("BlentT689-{}", std::process::id())
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
fn pump() {
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
#[test]
fn t689_native_owned_window_receives_mouse_and_touch_and_releases_on_drop() {
    let snapshot = NativeInventory.snapshot().unwrap();
    let monitor = &snapshot.monitors()[0];
    let (x, y) = monitor.project(0.5, 0.5).unwrap();
    let _window = Window::new(x, y);
    pump();
    let config = Config {
        monitor: monitor.id.clone(),
        mode: Mode::DirectMouse,
    };
    let mut mouse = Session::new(
        NativeInput::new(Mode::DirectMouse).unwrap(),
        &config,
        &snapshot,
    )
    .unwrap();
    let before = DOWN.load(Ordering::SeqCst);
    for button in [Button::Left, Button::Right, Button::Middle] {
        mouse
            .apply(
                &snapshot,
                Event::Mouse {
                    x: 0.5,
                    y: 0.5,
                    button: Some(button),
                    phase: Phase::Down,
                },
            )
            .unwrap();
        pump();
        mouse
            .apply(
                &snapshot,
                Event::Mouse {
                    x: 0.51,
                    y: 0.51,
                    button: None,
                    phase: Phase::Move,
                },
            )
            .unwrap();
        pump();
        mouse
            .apply(
                &snapshot,
                Event::Mouse {
                    x: 0.51,
                    y: 0.51,
                    button: Some(button),
                    phase: Phase::Up,
                },
            )
            .unwrap();
        pump();
    }
    assert_eq!(DOWN.load(Ordering::SeqCst) - before, 3);
    mouse
        .apply(
            &snapshot,
            Event::Mouse {
                x: 0.5,
                y: 0.5,
                button: Some(Button::Left),
                phase: Phase::Down,
            },
        )
        .unwrap();
    pump();
    let releases = UP.load(Ordering::SeqCst);
    drop(mouse);
    pump();
    assert_eq!(UP.load(Ordering::SeqCst), releases + 1);
    assert!(unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON)) } >= 0);
    native_touch(&snapshot, monitor.id.clone());
}
fn native_touch(snapshot: &crate::input_mapping::Snapshot, id: String) {
    let config = Config {
        monitor: id,
        mode: Mode::Touch,
    };
    let mut touch =
        Session::new(NativeInput::new(Mode::Touch).unwrap(), &config, snapshot).unwrap();
    let before = TOUCH.load(Ordering::SeqCst);
    for phase in [Phase::Down, Phase::Move, Phase::Up, Phase::Down] {
        touch
            .apply(
                snapshot,
                Event::Touch {
                    x: 0.5,
                    y: 0.5,
                    slot: 0,
                    phase,
                },
            )
            .unwrap();
        pump();
    }
    drop(touch);
    pump();
    assert!(
        TOUCH.load(Ordering::SeqCst) > before,
        "T689: owned window received no touch pointer events"
    );
}
#[test]
fn t689_native_contract_rejects_unowned_buttons_and_malformed_frames() {
    let mut mouse = NativeInput::new(Mode::DirectMouse).unwrap();
    for button in [Button::Left, Button::Right, Button::Middle] {
        assert!(mouse.mouse_flags(MouseAction::Up(button)).is_err());
    }
    assert!(mouse.touch(&[]).is_err());
    for count in [0, 2, u32::MAX] {
        assert!(accepted(count).is_err());
    }
    accepted(1).unwrap();
    let contact = Contact {
        slot: 0,
        position: (-100, -200),
        phase: Phase::Down,
    };
    for phase in [Phase::Down, Phase::Move, Phase::Up, Phase::Cancel] {
        let native = touch_info(&Contact { phase, ..contact });
        assert_eq!(
            unsafe { native.Anonymous.touchInfo.pointerInfo.ptPixelLocation.x },
            -100
        );
    }
    let mut touch = NativeInput::new(Mode::Touch).unwrap();
    for frame in [
        vec![],
        vec![contact; 11],
        vec![contact; 2],
        vec![Contact {
            slot: 255,
            ..contact
        }],
    ] {
        assert!(touch.touch(&frame).is_err());
    }
    touch.retire().unwrap();
    touch.retire().unwrap();
}

#[test]
fn t689_native_partial_session_creation_drops_its_device() {
    let snapshot = NativeInventory.snapshot().unwrap();
    let config = Config {
        monitor: "missing-owned-fixture-monitor".into(),
        mode: Mode::Touch,
    };
    for _ in 0..16 {
        let adapter = NativeInput::new(Mode::Touch).unwrap();
        assert!(Session::new(adapter, &config, &snapshot).is_err());
    }
    let mut adapter = NativeInput::new(Mode::Touch).unwrap();
    adapter.retire().unwrap();
}
