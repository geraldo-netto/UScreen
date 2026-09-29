use super::*;
use crate::input_mapping::{Monitor, Rect, Rotation};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Observed {
    touches: Vec<Vec<Contact>>,
    mouse: Vec<MouseAction>,
    retire: usize,
    fail: bool,
    cleanup_fail: bool,
}
struct Fake(Arc<Mutex<Observed>>);
impl Adapter for Fake {
    fn touch(&mut self, frame: &[Contact]) -> Result<()> {
        let mut s = self.0.lock().unwrap();
        ensure!(!s.fail, "denied");
        s.touches.push(frame.into());
        Ok(())
    }
    fn mouse(&mut self, _: (u16, u16), action: MouseAction) -> Result<()> {
        let mut s = self.0.lock().unwrap();
        ensure!(!s.fail, "denied");
        s.mouse.push(action);
        Ok(())
    }
    fn retire(&mut self) -> Result<()> {
        let mut s = self.0.lock().unwrap();
        s.retire += 1;
        ensure!(!s.cleanup_fail, "cleanup denied");
        Ok(())
    }
}
fn fixture(mode: Mode) -> (Session<Fake>, Snapshot, Arc<Mutex<Observed>>) {
    let snapshot = Snapshot::new(vec![Monitor {
        id: "m".into(),
        name: "m".into(),
        bounds: Rect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        },
        rotation: Rotation::Identity,
        scale_percent: 150,
        primary: true,
    }])
    .unwrap();
    let observed = Arc::new(Mutex::new(Observed::default()));
    let session = Session::new(
        Fake(observed.clone()),
        &Config {
            monitor: "m".into(),
            mode,
        },
        &snapshot,
    )
    .unwrap();
    (session, snapshot, observed)
}
fn touch(slot: u8, phase: Phase) -> Event {
    Event::Touch {
        x: 0.5,
        y: 0.5,
        slot,
        phase,
    }
}
fn mouse(button: Option<Button>, phase: Phase) -> Event {
    Event::Mouse {
        x: 0.5,
        y: 0.5,
        button,
        phase,
    }
}
#[test]
fn t689_touch_owns_slots_and_release_uses_last_position() {
    let (mut session, snapshot, observed) = fixture(Mode::Touch);
    for slot in 0..10 {
        session.apply(&snapshot, touch(slot, Phase::Down)).unwrap();
    }
    session.apply(&snapshot, touch(0, Phase::Move)).unwrap();
    session
        .apply(
            &snapshot,
            Event::Touch {
                x: 0.0,
                y: 0.0,
                slot: 0,
                phase: Phase::Up,
            },
        )
        .unwrap();
    let s = observed.lock().unwrap();
    assert_eq!(s.touches[9].len(), 10);
    assert_eq!(
        s.touches.last().unwrap().last().unwrap().position,
        (-961, 540)
    );
    drop(s);
    session.apply(&snapshot, touch(1, Phase::Cancel)).unwrap();
    session.retire().unwrap();
    assert!(session.apply(&snapshot, touch(2, Phase::Move)).is_err());
    assert_eq!(observed.lock().unwrap().retire, 1);
    drop(session);
    assert_eq!(observed.lock().unwrap().retire, 2);
}
#[test]
fn t689_direct_mouse_taps_right_click_and_drag_have_owned_transitions() {
    let (mut session, snapshot, observed) = fixture(Mode::DirectMouse);
    session.apply(&snapshot, mouse(None, Phase::Move)).unwrap();
    for button in [Button::Left, Button::Right, Button::Middle] {
        session
            .apply(&snapshot, mouse(Some(button), Phase::Down))
            .unwrap();
        assert!(session
            .apply(&snapshot, mouse(Some(button), Phase::Down))
            .is_err());
        session.apply(&snapshot, mouse(None, Phase::Move)).unwrap();
        session
            .apply(&snapshot, mouse(Some(button), Phase::Up))
            .unwrap();
        assert!(session
            .apply(&snapshot, mouse(Some(button), Phase::Up))
            .is_err());
    }
    session
        .apply(&snapshot, mouse(Some(Button::Left), Phase::Down))
        .unwrap();
    session
        .apply(&snapshot, mouse(Some(Button::Left), Phase::Cancel))
        .unwrap();
    assert_eq!(observed.lock().unwrap().mouse.len(), 12);
    session.retire().unwrap();
}
#[test]
fn t689_invalid_events_cannot_reach_adapter_or_create_contacts() {
    let (mut session, snapshot, observed) = fixture(Mode::Touch);
    for slot in 0..=u8::MAX {
        assert!(session.apply(&snapshot, touch(slot, Phase::Move)).is_err());
        if slot >= 10 {
            assert!(session.apply(&snapshot, touch(slot, Phase::Down)).is_err());
        }
    }
    for bad in [f64::NAN, f64::INFINITY, -1.0, 1.0 + f64::EPSILON] {
        assert!(session
            .apply(
                &snapshot,
                Event::Touch {
                    x: bad,
                    y: 0.5,
                    slot: 0,
                    phase: Phase::Down
                }
            )
            .is_err());
    }
    assert!(session.apply(&snapshot, mouse(None, Phase::Move)).is_err());
    assert!(observed.lock().unwrap().touches.is_empty());
    let (mut session, snapshot, observed) = fixture(Mode::DirectMouse);
    assert!(session.apply(&snapshot, touch(0, Phase::Down)).is_err());
    for phase in [Phase::Down, Phase::Up, Phase::Cancel] {
        assert!(session.apply(&snapshot, mouse(None, phase)).is_err());
    }
    assert!(observed.lock().unwrap().mouse.is_empty());
    for json in [
        r#"{"type":"pen"}"#,
        r#"{"type":"mouse","x":0,"y":0,"phase":"move","button":"eraser"}"#,
        r#"{"type":"touch","x":0,"y":0,"phase":"down","slot":256}"#,
    ] {
        assert!(serde_json::from_str::<Event>(json).is_err());
    }
    let restored: Config =
        serde_json::from_str(r#"{"monitor":"m","mode":"direct_mouse"}"#).unwrap();
    assert_eq!(restored.mode, Mode::DirectMouse);
    assert!(Session::new(
        Fake(observed),
        &Config {
            monitor: "missing".into(),
            mode: Mode::Touch
        },
        &snapshot
    )
    .is_err());
}
#[test]
fn t689_denied_partial_creation_replacement_and_shutdown_release_ownership() {
    for mode in [Mode::Touch, Mode::DirectMouse] {
        let (mut session, snapshot, observed) = fixture(mode);
        let event = match mode {
            Mode::Touch => touch(0, Phase::Down),
            Mode::DirectMouse => mouse(Some(Button::Left), Phase::Down),
        };
        observed.lock().unwrap().fail = true;
        assert!(session.apply(&snapshot, event).is_err());
        assert!(session.apply(&snapshot, event).is_err());
        assert_eq!(observed.lock().unwrap().retire, 1);
        observed.lock().unwrap().cleanup_fail = true;
        assert!(session.retire().is_err());
        observed.lock().unwrap().cleanup_fail = false;
        session.retire().unwrap();
    }
    let (mut session, snapshot, observed) = fixture(Mode::Touch);
    session.apply(&snapshot, touch(0, Phase::Down)).unwrap();
    let mut changed = snapshot.monitors().to_vec();
    changed[0].scale_percent = 200;
    assert!(session
        .apply(&Snapshot::new(changed).unwrap(), touch(0, Phase::Move))
        .is_err());
    assert_eq!(observed.lock().unwrap().retire, 1);
    let replacement = Session::new(
        Fake(observed.clone()),
        &Config {
            monitor: "m".into(),
            mode: Mode::Touch,
        },
        &snapshot,
    )
    .unwrap();
    drop(replacement);
    assert_eq!(observed.lock().unwrap().retire, 2);
}

#[test]
fn t673_preferences_validate_monitor_identity_modes_and_device_masks() {
    for monitor in [
        "".into(),
        "screen".into(),
        "x".repeat(1024),
        "x".repeat(1025),
        "screen\n".into(),
        "screen\0".into(),
    ] {
        for mode in [Mode::Touch, Mode::DirectMouse] {
            for (touch, mouse) in [(false, false), (true, false), (false, true), (true, true)] {
                let config = Config {
                    monitor: monitor.clone(),
                    mode,
                };
                let identity = !monitor.is_empty()
                    && monitor.len() <= 1024
                    && !monitor.chars().any(char::is_control);
                let enabled = if mode == Mode::Touch { touch } else { mouse };
                assert_eq!(config.validate(touch, mouse).is_ok(), identity && enabled);
                let file = crate::FileConfig {
                    direct_input: Some(config),
                    input_touch: touch,
                    input_mouse: mouse,
                    input_pen: false,
                    ..Default::default()
                };
                assert_eq!(file.validate_input_mode().is_ok(), identity && enabled);
            }
        }
    }
}
