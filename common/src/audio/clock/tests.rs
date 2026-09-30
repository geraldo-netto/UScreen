use super::*;
use crate::audio::{
    AudioCapabilities, AudioProfile, AudioSession, Direction, FrameWriter, PcmBlock, PcmQueue,
};
fn sample(epoch: u64, frames: u64, nanos: u64) -> ClockSample {
    ClockSample {
        epoch,
        frames,
        nanos,
    }
}
fn block(clock: Option<ClockSample>) -> PcmBlock {
    let mut block = PcmBlock::from_le_bytes(Direction::Microphone, &[1; 960]).unwrap();
    block.clock = clock;
    block
}
fn grant(clocked: bool) -> crate::audio::AudioGrant {
    let mut session =
        AudioSession::with_entropy(Direction::Microphone, crate::credentials::tests::entropy);
    let caps = AudioCapabilities {
        microphone: true,
        speech: true,
        ..Default::default()
    };
    let grant = session
        .start_with_clock(
            AudioProfile::new(Direction::Microphone),
            caps,
            true,
            0,
            clocked,
        )
        .unwrap();
    session
        .connected(grant.generation(), &grant.hello(), 0)
        .unwrap();
    grant
}
#[test]
fn t720_clock_encoding_has_fixed_bounds_and_explicit_unavailability() {
    assert_eq!(
        ClockSample::decode(&ClockSample::encode(None).unwrap()).unwrap(),
        None
    );
    for valid in [
        sample(1, 0, 1),
        sample(i64::MAX as u64, i64::MAX as u64, i64::MAX as u64),
    ] {
        assert_eq!(
            ClockSample::decode(&ClockSample::encode(Some(valid)).unwrap()).unwrap(),
            Some(valid)
        );
    }
    for length in 0..64 {
        if length != 24 {
            assert!(ClockSample::decode(&vec![0; length]).is_err());
        }
    }
    for invalid in [
        sample(0, 1, 1),
        sample(1, u64::MAX, 1),
        sample(u64::MAX, 1, 1),
        sample(1, 1, 0),
        sample(1, 1, u64::MAX),
    ] {
        assert!(!invalid.valid());
        assert!(ClockSample::encode(Some(invalid)).is_err());
        let mut bytes = [0; 24];
        bytes[..8].copy_from_slice(&invalid.epoch.to_be_bytes());
        bytes[8..16].copy_from_slice(&invalid.frames.to_be_bytes());
        bytes[16..].copy_from_slice(&invalid.nanos.to_be_bytes());
        assert!(ClockSample::decode(&bytes).is_err());
    }
}
#[test]
fn t720_clocked_wire_is_negotiated_and_invalid_counters_do_not_advance_sequence() {
    let clock = sample(9, 1234, 99999);
    let grant = grant(true);
    assert_eq!(&grant.hello()[..8], b"BLAUD002");
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    let mut writer = FrameWriter::new(grant.clone());
    assert!(writer.encode(&block(Some(sample(0, 1, 1))), 1).is_err());
    let packet = writer.encode(&block(Some(clock)), 1).unwrap();
    assert_eq!(packet.len(), 1012);
    for size in 0..packet.len() {
        assert!(reader.decode(&packet[..size]).is_err());
    }
    let mut invalid = packet.clone();
    invalid[28..36].fill(0);
    assert!(reader.decode(&invalid).is_err());
    assert!(reader.decode(&[packet.as_slice(), &[0]].concat()).is_err());
    let decoded = reader.decode(&packet).unwrap();
    assert_eq!(decoded.clock, Some(clock));
    assert_eq!(decoded.samples(), block(None).samples());
    assert_eq!(
        reader
            .decode(&writer.encode(&block(None), 2).unwrap())
            .unwrap()
            .clock,
        None
    );
    let old = self::grant(false);
    assert_eq!(&old.hello()[..8], b"BLAUD001");
    assert!(old.authenticate(&grant.hello()).is_err());
    assert!(grant.authenticate(&old.hello()).is_err());
    assert!(old
        .authenticate(&old.hello())
        .unwrap()
        .decode(&packet)
        .is_err());
}
#[test]
fn t720_counter_windows_reject_resets_stalls_outliers_and_stale_measurements() {
    let first = sample(1, 0, 1);
    let mut window = Window::default();
    assert!(!window.observe(None, 0));
    assert!(!window.observe(Some(sample(0, 0, 0)), 0));
    assert!(!window.observe(Some(first), 0));
    assert!(!window.observe(Some(first), 1));
    assert!(window.rate(1).is_none());
    assert!(!window.observe(Some(sample(1, 96000, 2_000_000_001)), 2000));
    assert_eq!(window.rate(2000), Some((96000, 2_000_000_000)));
    assert!(window.rate(1999).is_none());
    assert!(window.rate(5001).is_none());
    // Tiny fresh-looking counter movements cannot keep an old rate alive.
    assert!(!window.observe(Some(sample(1, 96001, 2_000_000_002)), 6000));
    assert!(window.rate(6000).is_none());
    for bad in [
        sample(2, 1, 3_000_000_001),
        sample(1, 1, 3_000_000_001),
        sample(1, 96000, 1),
    ] {
        let mut window = Window::default();
        window.observe(Some(sample(1, 96000, 2_000_000_001)), 0);
        assert!(window.observe(Some(bad), 1));
        assert!(window.rate(1).is_none());
    }
    for bad in [
        sample(1, 0, 2_000_000_001),
        sample(1, 480001, 2_000_000_001),
        sample(1, 96000, 5_000_000_002),
    ] {
        let mut window = Window::default();
        window.observe(Some(first), 0);
        assert!(window.observe(Some(bad), 5000));
        assert!(window.rate(5000).is_none());
    }
}
#[test]
fn t720_native_windows_drive_bounded_resampling_without_cross_clock_subtraction() {
    let mut profile = AudioProfile::new(Direction::Microphone);
    profile.buffer_ms = 20;
    let mut queue = PcmQueue::new(profile).unwrap();
    let mut output = [0; 480];
    for tick in 0..=600u64 {
        let now = tick * 10;
        let source = sample(
            1,
            tick * 480 + tick * 24 / 100,
            1_000_000_001 + now * 1_000_000,
        );
        let destination = sample(8, tick * 480, 900_000_000_001 + now * 1_000_000);
        queue.push(block(Some(source)), now).unwrap();
        queue.native_clock(Some(destination), now);
        queue.render(now, &mut output).unwrap();
    }
    assert_eq!(queue.measured_drift(), Some(500));
    assert!(output.iter().all(|value| *value == 257));
    queue.native_clock(None, 6001);
    assert_eq!(queue.measured_drift(), None);
    queue.native_clock(Some(sample(8, 0, 1)), 6002);
    queue.native_clock(Some(sample(9, 0, 2)), 6003);
    assert_eq!(queue.queued_frames(), 0);
    assert!(queue.render(6003, &mut output).unwrap().discontinuity);
    queue.push(block(Some(sample(2, 0, 1))), 6004).unwrap();
    queue.push(block(Some(sample(3, 0, 2))), 6005).unwrap();
    assert_eq!(queue.queued_frames(), 480);
    queue.native_clock(None, 1); // Invalid local observation time clears safely.
    assert_eq!(queue.queued_frames(), 0);
}
#[test]
fn t720_fractional_excess_native_drift_is_rejected_before_rounding() {
    for (extra_ns, measured) in [(0, Some(1000)), (1, None)] {
        let mut queue = PcmQueue::new(AudioProfile::new(Direction::Microphone)).unwrap();
        queue.push(block(Some(sample(1, 0, 1))), 0).unwrap();
        queue.native_clock(Some(sample(2, 0, 1)), 0);
        queue
            .push(block(Some(sample(1, 96096, 2_000_000_001))), 2000)
            .unwrap();
        queue.native_clock(Some(sample(2, 96000, 2_000_000_001 + extra_ns)), 2000);
        assert_eq!(queue.measured_drift(), measured);
        if measured.is_none() {
            assert_eq!(queue.queued_frames(), 0);
        }
    }
}
