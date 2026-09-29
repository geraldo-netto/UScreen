use super::*;
use crate::{
    direct_input::{Config, Event, Session},
    input_mapping::MonitorInventory,
    windows::monitors::NativeInventory,
};
use std::sync::atomic::Ordering;
#[path = "../../../../testdata/input_window.rs"]
mod window;
use window::{pump, Window, DOWN, TOUCH, UP};

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
