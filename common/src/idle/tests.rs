use super::*;

#[test]
fn t492_timestamp_conversion_is_bounded_and_preserves_wall_time_gaps() {
    assert_eq!(timestamp_us(15, 1, 30), Some(500_000));
    assert_eq!(timestamp_us(-1, 1, 1_000_000), Some(-1));
    for invalid in [i128::MIN, i128::MAX, i128::from(i64::MAX)] {
        assert_eq!(timestamp_us(invalid, u32::MAX, 1), None);
    }
    assert_eq!(timestamp_us(0, 0, 1), None);
    assert_eq!(timestamp_us(0, 1, 0), None);
    for value in -2048..2048 {
        assert_eq!(timestamp_us(value, 1, 1_000_000), Some(value as i64));
    }
}

fn sample(sequence: u32, pts_us: i64) -> Sample {
    Sample {
        sequence,
        pts_us,
        ready_us: pts_us as u64 + 100_000,
        ack_us: pts_us as u64 + 110_000,
        keyframe: sequence.is_multiple_of(2),
    }
}

fn trial() -> (Policy, Sample) {
    let mut policy = Policy::new(true);
    let mut frame = sample(0, 0);
    for index in 0..=WINDOW {
        frame = sample(index as u32, index as i64 * 200_000);
        policy.observe(frame);
    }
    assert_eq!(policy.phase, Phase::Trial);
    (policy, frame)
}

fn next(frame: Sample, interval: i64) -> Sample {
    sample(frame.sequence.wrapping_add(1), frame.pts_us + interval)
}

#[test]
fn t492_current_profile_needs_measured_baseline_and_sparse_trial() {
    assert_eq!(Policy::new(false).interval_ms(), COMPATIBLE_MS);
    let (mut policy, mut frame) = trial();
    assert_eq!(policy.interval_ms(), SPARSE_MS);
    for _ in 0..WINDOW {
        frame = next(frame, 500_000);
        policy.observe(frame);
    }
    assert_eq!(policy.phase, Phase::Active);
    for _ in 0..30 {
        frame = next(frame, 33_334);
        policy.observe(frame);
    }
    assert_eq!(
        policy.phase,
        Phase::Active,
        "fresh motion must remain responsive"
    );
    policy.tick(frame.ack_us + 1_500_001);
    assert_eq!(policy.interval_ms(), COMPATIBLE_MS);
    policy.observe(next(frame, 500_000));
    assert_eq!(
        policy.phase,
        Phase::Rejected,
        "no retries within a failed epoch"
    );
    assert_eq!(
        Policy::new(true).interval_ms(),
        COMPATIBLE_MS,
        "restart needs fresh evidence"
    );
}

#[test]
fn t492_slow_ack_encoder_queue_growth_and_clock_steps_restore_compatibility() {
    for offset in [30_000i64, 500_000, -30_000] {
        let (mut policy, previous) = trial();
        let mut frame = next(previous, 500_000);
        frame.ready_us = frame.ready_us.checked_add_signed(offset).unwrap();
        frame.ack_us = frame.ack_us.checked_add_signed(offset).unwrap();
        policy.observe(frame);
        assert_eq!(policy.phase, Phase::Rejected);
    }
    let (mut policy, previous) = trial();
    let mut frame = next(previous, 500_000);
    frame.ack_us += 150_000;
    policy.observe(frame);
    assert_eq!(policy.phase, Phase::Rejected);
}

#[test]
fn t492_missing_frames_keys_and_invalid_samples_cannot_certify_idle() {
    for mutation in 0..5 {
        let (mut policy, previous) = trial();
        let mut frame = next(previous, 500_000);
        match mutation {
            0 => frame.sequence += 1,
            1 => frame.pts_us = -1,
            2 => frame.pts_us = previous.pts_us,
            3 => frame.ack_us = frame.ready_us - 1,
            _ => frame.ready_us = previous.ready_us - 1,
        }
        policy.observe(frame);
        assert_eq!(policy.phase, Phase::Rejected);
    }
    let (mut policy, mut frame) = trial();
    for _ in 0..4 {
        frame = next(frame, 500_000);
        frame.keyframe = false;
        policy.observe(frame);
    }
    assert_eq!(policy.phase, Phase::Rejected);
}

#[test]
fn t492_busy_or_unmeasured_stream_remains_at_compatible_cadence() {
    let mut policy = Policy::new(true);
    for index in 0..100 {
        policy.observe(sample(index, i64::from(index) * 33_334));
    }
    assert_eq!(policy.phase, Phase::Baseline);
    let mut policy = Policy::new(true);
    for index in 0..100 {
        let mut frame = sample(index, i64::from(index) * 200_000);
        frame.keyframe = false;
        policy.observe(frame);
    }
    assert_eq!(policy.phase, Phase::Baseline);
    let mut policy = Policy::new(true);
    for index in 0..100 {
        let mut frame = sample(index, i64::from(index) * 200_000);
        frame.ack_us += 150_000;
        policy.observe(frame);
    }
    assert_eq!(policy.phase, Phase::Baseline);
    let (mut policy, mut frame) = trial();
    for _ in 0..40 {
        frame = next(frame, 33_334);
        policy.observe(frame);
    }
    assert_eq!(policy.phase, Phase::Baseline);
    policy.tick(u64::MAX);
    assert_eq!(policy.interval_ms(), COMPATIBLE_MS);
}

#[test]
fn t492_active_epoch_rejects_regression_and_sequence_wrap_is_valid() {
    let (mut policy, mut frame) = trial();
    for _ in 0..WINDOW {
        frame = next(frame, 500_000);
        policy.observe(frame);
    }
    frame = next(frame, 500_000);
    frame.ack_us += 50_000;
    policy.observe(frame);
    assert_eq!(policy.phase, Phase::Rejected);
    assert!(valid(sample(0, 2), Some(sample(u32::MAX, 1))));
    assert!(!valid(
        Sample {
            ack_us: 109_999,
            ..sample(1, 2)
        },
        Some(sample(0, 1))
    ));
}

// T667: independently exercise timing predicates and admission boundaries.
fn measured_trial(latency: impl Fn(u32) -> u64) -> (Policy, Sample) {
    let mut policy = Policy::new(true);
    let mut frame = sample(0, 0);
    for index in 0..=WINDOW as u32 {
        frame = sample(index, i64::from(index) * 200_000);
        frame.ack_us = frame.ready_us + latency(index);
        policy.observe(frame);
        let expected = if index == WINDOW as u32 {
            Phase::Trial
        } else {
            Phase::Baseline
        };
        assert_eq!(policy.phase, expected, "T667: admission at sample {index}");
    }
    (policy, frame)
}

#[test]
fn t667_tick_preserves_baseline_and_exact_sparse_deadline() {
    let mut baseline = Policy::new(true);
    baseline.observe(sample(0, 0));
    baseline.tick(u64::MAX);
    assert_eq!(baseline.phase, Phase::Baseline);
    let (mut policy, frame) = trial();
    for elapsed in [0, 1_499_999, 1_500_000] {
        policy.tick(frame.ack_us + elapsed);
        assert_eq!(policy.phase, Phase::Trial, "T667: elapsed {elapsed}");
    }
    policy.tick(frame.ack_us + 1_500_001);
    assert_eq!(policy.phase, Phase::Rejected);
}

#[test]
fn t667_baseline_ack_limit_is_inclusive_and_never_admits_slow_frames() {
    for latency in [0, 1, 99_999, 100_000] {
        measured_trial(|_| latency);
    }
    for latency in [100_001, 150_000, 1_000_000] {
        let mut policy = Policy::new(true);
        for index in 0..40 {
            let mut frame = sample(index, i64::from(index) * 200_000);
            frame.ack_us = frame.ready_us + latency;
            policy.observe(frame);
            assert_eq!(policy.phase, Phase::Baseline, "T667: slow frame {index}");
        }
    }
}

#[test]
fn t667_trial_waits_for_the_complete_sample_window() {
    let (mut policy, mut frame) = trial();
    for index in 1..=WINDOW {
        frame = next(frame, 500_000);
        policy.observe(frame);
        let expected = if index == WINDOW {
            Phase::Active
        } else {
            Phase::Trial
        };
        assert_eq!(policy.phase, expected, "T667: trial sample {index}");
    }
}

#[test]
fn t667_pending_cadence_grace_expires_at_one_second() {
    let (mut policy, previous) = trial();
    let pending = next(previous, 999_999);
    policy.observe(pending);
    assert_eq!(policy.phase, Phase::Trial);
    policy.observe(next(pending, 1));
    assert_eq!(policy.phase, Phase::Baseline);
}

#[test]
fn t667_ack_limit_is_independent_of_relative_cost_margin() {
    for (latency, expected) in [(100_000, Phase::Trial), (100_001, Phase::Rejected)] {
        let (mut policy, previous) = measured_trial(|_| 90_000);
        let mut frame = next(previous, 500_000);
        frame.ack_us = frame.ready_us + latency;
        policy.observe(frame);
        assert_eq!(policy.phase, expected, "T667: trial ACK latency {latency}");
    }
}

#[test]
fn t667_percentile_uses_the_ordered_window_and_exact_cost_margin() {
    // Sixteen costs descending from 26ms to 11ms have a 95th percentile of 26ms.
    for (latency, expected) in [
        (6_000, Phase::Trial),
        (46_000, Phase::Trial),
        (5_999, Phase::Rejected),
        (46_001, Phase::Rejected),
    ] {
        let (mut policy, previous) = measured_trial(|index| 27_000 - u64::from(index) * 1_000);
        let mut frame = next(previous, 500_000);
        frame.ack_us = frame.ready_us + latency;
        policy.observe(frame);
        assert_eq!(
            policy.phase, expected,
            "T667: percentile margin at {latency}"
        );
    }
}
