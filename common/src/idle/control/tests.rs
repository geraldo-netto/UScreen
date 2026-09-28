use super::*;
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct State {
    interval: Cell<u32>,
    fail: Cell<bool>,
}
struct Adapter(Rc<State>);
impl Cadence for Adapter {
    fn publish(&self, interval_ms: u32) -> Result<()> {
        anyhow::ensure!(!self.0.fail.get(), "T695 adapter unavailable");
        self.0.interval.set(interval_ms);
        Ok(())
    }
    fn clear(&self) {
        self.0.interval.set(COMPATIBLE_MS);
    }
}
const EPOCH: Epoch = Epoch {
    encoder: 1,
    decoder: 2,
    viewer: 3,
};
fn control() -> (Control<Adapter>, Rc<State>) {
    let state = Rc::new(State::default());
    (Control::new(Adapter(state.clone()), EPOCH, true), state)
}
fn frame(sequence: u32, pts: u64) -> Encoded {
    Encoded {
        sequence,
        pts_us: Some(pts as i64),
        ready_us: pts + 100_000,
        keyframe: sequence.is_multiple_of(2),
    }
}
fn feed<A: Cadence>(control: &mut Control<A>, sequence: u32, pts: u64) {
    control.encoded(EPOCH, frame(sequence, pts));
    control.acknowledge(EPOCH, u64::from(sequence) + 1, sequence, pts + 110_000);
    control.tick(EPOCH, true, pts + 110_000).unwrap();
}
fn trial(control: &mut Control<Adapter>) {
    for sequence in 0..=16 {
        feed(control, sequence, u64::from(sequence) * 200_000);
    }
    assert_eq!(control.phase(), Phase::Trial);
}
#[test]
fn t695_epoch_local_trial_motion_keys_staleness_and_owned_drop() {
    let (mut control, state) = control();
    trial(&mut control);
    assert_eq!(state.interval.get(), 500);
    for sequence in 17..=32 {
        feed(
            &mut control,
            sequence,
            3_200_000 + u64::from(sequence - 16) * 500_000,
        );
    }
    assert_eq!(control.phase(), Phase::Active);
    feed(&mut control, 33, 11_233_334);
    assert_eq!(
        control.phase(),
        Phase::Active,
        "motion preserves fresh evidence"
    );
    control.tick(EPOCH, true, 12_843_335).unwrap();
    assert_eq!(control.phase(), Phase::Rejected);
    assert_eq!(state.interval.get(), 200);
    let (mut control, state) = self::control();
    trial(&mut control);
    drop(control);
    assert_eq!(state.interval.get(), 200);
}
#[test]
fn t695_retirement_and_adapter_failure_clear_sparse_request() {
    for mode in 0..5 {
        let (mut control, state) = control();
        trial(&mut control);
        let mut epoch = EPOCH;
        match mode {
            0 => epoch.encoder += 1,
            1 => epoch.decoder += 1,
            2 => epoch.viewer += 1,
            3 => {}
            _ => state.fail.set(true),
        }
        let result = control.tick(epoch, mode != 3, 3_310_000);
        assert_eq!(result.is_err(), mode == 4);
        assert_eq!(control.phase(), Phase::Rejected);
        assert_eq!(state.interval.get(), 200);
    }
}
#[test]
fn t695_invalid_epochs_missing_and_malformed_evidence_fail_closed() {
    for value in 0..3 {
        let mut epoch = EPOCH;
        match value {
            0 => epoch.encoder = 0,
            1 => epoch.decoder = 0,
            _ => epoch.viewer = 0,
        }
        let control = Control::new(Adapter(Rc::default()), epoch, true);
        assert_eq!(control.phase(), Phase::Rejected);
    }
    assert_eq!(
        Control::new(Adapter(Rc::default()), EPOCH, false).phase(),
        Phase::Rejected
    );
    for mode in 0..8 {
        let (mut control, state) = control();
        trial(&mut control);
        let mut frame = frame(17, 3_700_000);
        let mut epoch = EPOCH;
        invalidate(mode, &mut frame, &mut epoch);
        control.encoded(epoch, frame);
        control.acknowledge(epoch, if mode == 5 { u64::MAX } else { 18 }, 17, 3_810_000);
        assert_eq!(control.phase(), Phase::Rejected, "mode {mode}");
        assert_eq!(state.interval.get(), 200);
    }
}
#[test]
fn t695_pending_bound_and_ack_ordinal_boundaries() {
    for count in [0, 1, 255, 256, 257, 512] {
        let (mut control, _) = control();
        for index in 0..count {
            control.encoded(EPOCH, frame(index, u64::from(index) * 200_000));
        }
        assert!(control.pending.len() <= 256);
        control.acknowledge(EPOCH, 1, 0, 110_000);
        assert_eq!(
            control.phase() == Phase::Rejected,
            count == 0 || count > 256
        );
    }
    for ordinal in (0..=256).chain([u64::MAX]) {
        let (mut control, _) = control();
        control.encoded(EPOCH, frame(0, 0));
        control.acknowledge(EPOCH, ordinal, 0, 110_000);
        assert_eq!(control.phase() == Phase::Rejected, ordinal != 1);
    }
    let (mut control, _) = control();
    control.ordinal = u64::MAX;
    control.acknowledge(EPOCH, 0, 0, 0);
    assert_eq!(control.phase(), Phase::Rejected);
    let (mut control, _) = self::control();
    control.encoded(EPOCH, frame(u32::MAX, 0));
    control.acknowledge(EPOCH, 1, u32::MAX, 110_000);
    control.encoded(EPOCH, frame(0, 200_000));
    control.acknowledge(EPOCH, 2, 0, 310_000);
    assert_eq!(control.phase(), Phase::Baseline);
}

fn invalidate(mode: u32, frame: &mut Encoded, epoch: &mut Epoch) {
    match mode {
        0 => frame.pts_us = None,
        1 => frame.pts_us = Some(-1),
        2 => frame.ready_us = u64::MAX,
        3 => frame.sequence = 18,
        4 => epoch.encoder += 1,
        5 => {}
        6 => epoch.decoder += 1,
        _ => frame.pts_us = Some(3_200_000),
    }
}
