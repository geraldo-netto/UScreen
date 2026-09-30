use super::*;

fn caps() -> AudioCapabilities {
    AudioCapabilities {
        microphone: true,
        speakers: true,
        speech: true,
        raw: true,
        background: true,
    }
}

fn grant(direction: Direction) -> AudioGrant {
    AudioGrant::new(
        AudioProfile::new(direction),
        1,
        crate::credentials::tests::entropy,
    )
    .unwrap()
}

fn block(direction: Direction, value: i16) -> PcmBlock {
    PcmBlock {
        samples: [value; MAX_SAMPLES],
        channels: direction.channels(),
        discontinuity: false,
    }
}

#[test]
fn t719_capture_transfer_bounds_age_overflow_and_partial_rendering() {
    let profile = AudioProfile::new(Direction::Speakers);
    let mut queue = PcmQueue::new(profile).unwrap();
    assert!(queue.pop(0).unwrap().is_none());
    for i in 0..21 {
        queue.push(block(Direction::Speakers, i), i as u64).unwrap();
    }
    let next = queue.pop(21).unwrap().unwrap();
    assert_eq!(next.samples()[0], 1);
    assert!(next.discontinuity);
    assert!(!queue.pop(22).unwrap().unwrap().discontinuity);
    assert!(queue.pop(21).is_err());
    assert!(queue.pop(221).unwrap().is_none());
    queue.push(block(Direction::Speakers, 9), 222).unwrap();
    assert!(queue.pop(222).unwrap().unwrap().discontinuity);
    assert!(queue.push(block(Direction::Microphone, 1), 222).is_err());
    for i in 0..4 {
        queue.push(block(Direction::Speakers, 1), 222 + i).unwrap();
    }
    queue.adjust_drift(48048, 48000).unwrap();
    queue.render(226, &mut [0; 960]).unwrap();
    queue.adjust_drift(48000, 48000).unwrap();
    assert!(queue.pop(226).is_err());
    queue.clear();
    queue.adjust_drift(48001, 48000).unwrap();
    assert!(queue.pop(226).is_err());
}

fn streaming(direction: Direction) -> (AudioSession, AudioGrant) {
    let mut session = AudioSession::with_entropy(direction, crate::credentials::tests::entropy);
    let grant = session
        .start(AudioProfile::new(direction), caps(), true, 0)
        .unwrap();
    session
        .connected(grant.generation(), &grant.hello(), 0)
        .unwrap();
    (session, grant)
}

#[test]
fn t717_profile_bounds_and_serialized_invalid_values() {
    let mut profile = AudioProfile::new(Direction::Microphone);
    assert_eq!(profile.buffer_ms, 40);
    assert_eq!(Processing::default(), Processing::Speech);
    for value in 0..=220 {
        profile.buffer_ms = value;
        assert_eq!(
            profile.validate().is_ok(),
            (20..=200).contains(&value) && value % 10 == 0
        );
    }
    profile.buffer_ms = u16::MAX;
    assert!(profile.validate().is_err());
    for value in [
        "-1",
        "65536",
        "18446744073709551616",
        "null",
        "1.5",
        "\"40\"",
    ] {
        let json = format!(
            r#"{{"direction":"microphone","processing":"speech","buffer_ms":{value},"background":false}}"#
        );
        assert!(serde_json::from_str::<AudioProfile>(&json).is_err());
    }
    let profile = AudioProfile::new(Direction::Speakers);
    assert_eq!(
        serde_json::from_str::<AudioProfile>(&serde_json::to_string(&profile).unwrap()).unwrap(),
        profile
    );
}

#[test]
fn t717_capabilities_fail_closed_per_direction_and_processing() {
    for direction in [Direction::Microphone, Direction::Speakers] {
        let mut profile = AudioProfile::new(direction);
        assert!(AudioCapabilities::default().validate(profile).is_err());
        assert!(caps().validate(profile).is_ok());
        let mut limited = caps();
        limited.speech = false;
        assert!(limited.validate(profile).is_err());
        profile.processing = Processing::Raw;
        assert!(limited.validate(profile).is_ok());
        limited.raw = false;
        assert!(limited.validate(profile).is_err());
        profile.background = true;
        limited = caps();
        limited.background = false;
        assert!(limited.validate(profile).is_err());
        assert!(caps().validate(profile).is_ok());
    }
}

#[test]
fn t717_handshake_rejects_every_changed_byte_and_truncation() {
    for direction in [Direction::Microphone, Direction::Speakers] {
        let grant = grant(direction);
        let hello = grant.hello();
        assert_eq!(grant.profile(), AudioProfile::new(direction));
        assert!(grant.authenticate(&hello).is_ok());
        for index in 0..HELLO_BYTES {
            let mut changed = hello;
            changed[index] ^= 0x80;
            assert!(grant.authenticate(&changed).is_err(), "byte {index}");
            assert!(grant.authenticate(&hello[..index]).is_err());
        }
        assert!(grant.authenticate(&[0; HELLO_BYTES + 1]).is_err());
        assert!(grant.authenticate(&self::grant(direction).hello()).is_err());
    }
}

#[test]
fn t717_pcm_roundtrip_preserves_signed_extremes_and_stereo_order() {
    for direction in [Direction::Microphone, Direction::Speakers] {
        let grant = grant(direction);
        let samples: Vec<i16> = [i16::MIN, -1, 0, 1, i16::MAX]
            .into_iter()
            .cycle()
            .take(BLOCK_FRAMES * direction.channels())
            .collect();
        let packet = grant.encode(0, 123, &samples).unwrap();
        let mut reader = grant.authenticate(&grant.hello()).unwrap();
        assert_eq!(
            reader.payload_bytes(&packet[..FRAME_HEADER_BYTES]).unwrap(),
            samples.len() * 2
        );
        let block = reader.decode(&packet).unwrap();
        assert_eq!(block.samples(), samples);
        assert!(!block.discontinuity);
        assert!(reader.decode(&packet).is_err());
        assert!(grant.encode(u64::MAX, 0, &samples).is_err());
        assert!(grant.encode(0, 0, &[]).is_err());
    }
}

#[test]
fn t717_frame_truncation_and_size_errors_do_not_advance_sequence() {
    let grant = grant(Direction::Speakers);
    let packet = grant.encode(0, 10, &[1; MAX_SAMPLES]).unwrap();
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    for length in 0..packet.len() {
        assert!(reader.decode(&packet[..length]).is_err());
    }
    let mut oversized = packet.clone();
    oversized.push(0);
    assert!(reader.decode(&oversized).is_err());
    for index in (0..8).chain(24..28) {
        let mut changed = packet.clone();
        changed[index] ^= 255;
        assert!(reader.decode(&changed).is_err());
    }
    for length in 0..FRAME_HEADER_BYTES {
        assert!(reader.payload_bytes(&packet[..length]).is_err());
    }
    assert!(reader
        .payload_bytes(&packet[..FRAME_HEADER_BYTES + 1])
        .is_err());
    assert_eq!(reader.decode(&packet).unwrap().samples(), &[1; MAX_SAMPLES]);
}

#[test]
fn t717_sequence_gaps_reordering_and_wrap() {
    let grant = grant(Direction::Microphone);
    let mut reader = grant.authenticate(&grant.hello()).unwrap();
    assert!(reader
        .decode(&grant.encode(1, 0, &[0; BLOCK_FRAMES]).unwrap())
        .is_err());
    reader
        .decode(&grant.encode(0, 0, &[0; BLOCK_FRAMES]).unwrap())
        .unwrap();
    assert!(reader
        .decode(&grant.encode(1, 0, &[0; BLOCK_FRAMES]).unwrap())
        .is_err());
    assert!(
        reader
            .decode(&grant.encode(2, 1, &[0; BLOCK_FRAMES]).unwrap())
            .unwrap()
            .discontinuity
    );
    assert!(reader
        .decode(&grant.encode(1, 2, &[0; BLOCK_FRAMES]).unwrap())
        .is_err());
    assert!(
        !reader
            .decode(&grant.encode(3, 2, &[0; BLOCK_FRAMES]).unwrap())
            .unwrap()
            .discontinuity
    );
    let mut packet = grant.encode(4, 3, &[0; BLOCK_FRAMES]).unwrap();
    packet[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
    assert!(reader.decode(&packet).is_err());
}

#[test]
fn t717_bounded_frame_mutations_never_escape_pcm_limits() {
    let grant = grant(Direction::Speakers);
    let packet = grant.encode(0, 0, &[i16::MIN; MAX_SAMPLES]).unwrap();
    let mut seed = 717u64;
    for _ in 0..2048 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut bytes = packet.clone();
        bytes[(seed as usize) % packet.len()] ^= (seed >> 32) as u8;
        let mut reader = grant.authenticate(&grant.hello()).unwrap();
        if let Ok(decoded) = reader.decode(&bytes) {
            assert_eq!(decoded.samples().len(), MAX_SAMPLES);
            assert!(!decoded.discontinuity);
        }
    }
}

#[test]
fn t717_queue_prefill_overflow_underflow_and_no_repeated_speech() {
    let mut queue = PcmQueue::new(AudioProfile::new(Direction::Microphone)).unwrap();
    let mut output = [77; BLOCK_FRAMES];
    for index in 0..3 {
        queue.push(block(Direction::Microphone, index), 0).unwrap();
        assert!(queue.render(0, &mut output).unwrap().silence);
        assert_eq!(output, [0; BLOCK_FRAMES]);
    }
    queue.push(block(Direction::Microphone, 3), 0).unwrap();
    for index in 0..4 {
        assert!(!queue.render(0, &mut output).unwrap().silence);
        assert_eq!(output, [index; BLOCK_FRAMES]);
    }
    assert!(queue.render(0, &mut output).unwrap().silence);
    assert_eq!(output, [0; BLOCK_FRAMES]);
    for index in 0..25 {
        queue.push(block(Direction::Microphone, index), 0).unwrap();
    }
    assert_eq!(queue.queued_frames(), 20 * BLOCK_FRAMES);
    assert!(queue.render(0, &mut output).unwrap().discontinuity);
    assert_eq!(output, [5; BLOCK_FRAMES]);
    queue.clear();
    assert_eq!(queue.queued_frames(), 0);
}

#[test]
fn t717_queue_rejects_channels_sizes_backwards_clock_and_expires_old_audio() {
    let mut queue = PcmQueue::new(AudioProfile::new(Direction::Speakers)).unwrap();
    assert!(queue.push(block(Direction::Microphone, 1), 0).is_err());
    queue.push(block(Direction::Speakers, 7), 10).unwrap();
    assert!(queue.push(block(Direction::Speakers, 1), 9).is_err());
    let mut small = [7; 2];
    assert!(queue.render(10, &mut small).is_err());
    assert_eq!(small, [0; 2]);
    let mut output = [7; MAX_SAMPLES];
    assert!(queue.render(9, &mut output).is_err());
    assert_eq!(output, [0; MAX_SAMPLES]);
    queue.render(210, &mut output).unwrap();
    assert_eq!(queue.queued_frames(), BLOCK_FRAMES);
    assert!(queue.render(211, &mut output).unwrap().discontinuity);
    assert_eq!(queue.queued_frames(), 0);
    let mut gap = block(Direction::Speakers, 4);
    gap.discontinuity = true;
    queue.push(block(Direction::Speakers, 3), 211).unwrap();
    queue.push(gap, 211).unwrap();
    assert_eq!(queue.queued_frames(), BLOCK_FRAMES);
}

#[test]
fn t717_drift_bounds_overflow_and_discontinuity() {
    let mut queue = PcmQueue::new(AudioProfile::new(Direction::Microphone)).unwrap();
    for frames in [
        998_999,
        999_000,
        999_999,
        1_000_000,
        1_001_000,
        1_001_001,
        u64::MAX,
    ] {
        queue.push(block(Direction::Microphone, 1), 0).unwrap();
        assert_eq!(
            queue.adjust_drift(frames, 1_000_000).is_ok(),
            (999_000..=1_001_000).contains(&frames)
        );
    }
    assert_eq!(queue.queued_frames(), 0);
    assert!(queue.adjust_drift(0, 1).is_err());
    assert!(queue.adjust_drift(1, 0).is_err());
    assert_eq!(queue.adjust_drift(u64::MAX, u64::MAX).unwrap(), 0);
}

#[test]
fn t717_resampling_preserves_channels_and_bounded_consumption() {
    for delta in [-1000, -1, 0, 1, 1000] {
        let mut queue = PcmQueue::new(AudioProfile::new(Direction::Speakers)).unwrap();
        for _ in 0..20 {
            let mut frame = block(Direction::Speakers, i16::MIN);
            for pair in frame.samples.chunks_exact_mut(2) {
                pair[1] = i16::MAX;
            }
            queue.push(frame, 0).unwrap();
        }
        queue
            .adjust_drift((1_000_000 + delta) as u64, 1_000_000)
            .unwrap();
        let mut output = [0; MAX_SAMPLES];
        for _ in 0..10 {
            assert!(!queue.render(0, &mut output).unwrap().silence);
            assert!(output
                .chunks_exact(2)
                .all(|pair| pair == [i16::MIN, i16::MAX]));
        }
        assert!((4795..=4805).contains(&queue.queued_frames()));
    }
}

#[test]
fn t717_session_rejects_implicit_unauthenticated_and_unsupported_start() {
    let direction = Direction::Microphone;
    let profile = AudioProfile::new(direction);
    let mut session = AudioSession::with_entropy(direction, crate::credentials::tests::entropy);
    assert_eq!(session.state(), AudioState::Stopped);
    assert_eq!(session.stop(false), None);
    assert_eq!(session.tick(u64::MAX), None);
    assert!(!session.retired(0));
    assert!(session.start(profile, caps(), false, 0).is_err());
    assert!(session
        .start(profile, AudioCapabilities::default(), true, 0)
        .is_err());
    assert!(session
        .start(AudioProfile::new(Direction::Speakers), caps(), true, 0)
        .is_err());
    assert!(session.start(profile, caps(), true, u64::MAX).is_err());
    let grant = session.start(profile, caps(), true, 0).unwrap();
    assert_eq!(session.state(), AudioState::Starting);
    assert!(session.start(profile, caps(), true, 0).is_err());
    assert!(session.connected(2, &grant.hello(), 0).is_err());
    assert!(session.connected(1, &[0; HELLO_BYTES], 0).is_err());
    assert_eq!(session.tick(4999), None);
    assert_eq!(session.tick(5000), Some(1));
    assert!(session.retired(1));
    assert_eq!(session.state(), AudioState::Failed);
}

#[test]
fn t717_both_directions_stop_and_retirement_are_independent() {
    let (mut mic, mg) = streaming(Direction::Microphone);
    let (mut speaker, sg) = streaming(Direction::Speakers);
    assert_eq!(mic.stop(false), Some(mg.generation()));
    assert_eq!(speaker.state(), AudioState::Streaming);
    assert_eq!(mic.stop(false), Some(mg.generation()));
    assert!(!mic.retired(9));
    assert!(mic.start(mg.profile(), caps(), true, 0).is_err());
    assert!(mic.retired(mg.generation()));
    assert!(!mic.retired(mg.generation()));
    let new = mic.start(mg.profile(), caps(), true, 0).unwrap();
    assert_eq!(new.generation(), 2);
    assert_ne!(new.hello(), mg.hello());
    assert!(mic.connected(1, &mg.hello(), 0).is_err());
    assert!(!mic.retired(1));
    mic.connected(2, &new.hello(), 0).unwrap();
    assert!(mic.receive(1, &[], 0).is_err());
    assert_eq!(mic.state(), AudioState::Streaming);
    speaker.stop(false);
    speaker.retired(sg.generation());
    assert_eq!(speaker.state(), AudioState::Stopped);
    assert_eq!(mic.state(), AudioState::Streaming);
}

#[test]
fn t717_receive_render_disconnect_and_fresh_start() {
    for direction in [Direction::Microphone, Direction::Speakers] {
        let (mut session, grant) = streaming(direction);
        let samples = vec![123; BLOCK_FRAMES * direction.channels()];
        let mut output = samples.clone();
        for index in 0..4 {
            session
                .receive(
                    1,
                    &grant.encode(index, index, &samples).unwrap(),
                    index * 10,
                )
                .unwrap();
        }
        assert!(!session.render(1, 30, &mut output).unwrap().silence);
        assert_eq!(output, samples);
        assert_eq!(session.adjust_drift(1, 48000, 48000).unwrap(), 0);
        assert!(session.adjust_drift(9, 1, 1).is_err());
        session.stop(true);
        assert!(session.receive(1, &[], 40).is_err());
        assert!(session.render(1, 40, &mut output).is_err());
        assert!(output.iter().all(|sample| *sample == 0));
        assert!(session.adjust_drift(1, 1, 1).is_err());
        assert!(session.retired(1));
        assert_eq!(session.state(), AudioState::Failed);
        let fresh = session.start(grant.profile(), caps(), true, 100).unwrap();
        session.connected(2, &fresh.hello(), 100).unwrap();
        assert!(session.render(2, 100, &mut output).unwrap().silence);
    }
}

#[test]
fn t717_malformed_receive_closes_admission_and_deadline_cannot_be_extended() {
    let (mut session, grant) = streaming(Direction::Microphone);
    assert!(session.connected(1, &grant.hello(), 0).is_err());
    assert!(session.receive(1, &[], 1).is_err());
    assert_eq!(session.state(), AudioState::Stopping);
    assert_eq!(session.tick(999), None);
    session.retired(1);
    let next = session.start(grant.profile(), caps(), true, 0).unwrap();
    assert!(session.connected(2, &next.hello(), 5000).is_err());
    session.connected(2, &next.hello(), 4999).unwrap();
    assert_eq!(session.tick(5248), None);
    assert_eq!(session.tick(5249), Some(2));
}

#[test]
fn t717_receive_and_render_enforce_deadline_without_poll() {
    let (mut session, grant) = streaming(Direction::Microphone);
    assert!(session
        .receive(1, &grant.encode(0, 0, &[1; BLOCK_FRAMES]).unwrap(), 250)
        .is_err());
    assert_eq!(session.state(), AudioState::Stopping);
    let (mut session, _) = streaming(Direction::Speakers);
    let mut output = [99; MAX_SAMPLES];
    assert!(session.render(1, 250, &mut output).is_err());
    assert_eq!(session.state(), AudioState::Stopping);
    assert_eq!(output, [0; MAX_SAMPLES]);
}

#[test]
fn t717_stale_render_cannot_retire_replacement() {
    let (mut session, grant) = streaming(Direction::Microphone);
    session.stop(false);
    session.retired(1);
    let new = session.start(grant.profile(), caps(), true, 0).unwrap();
    session.connected(2, &new.hello(), 0).unwrap();
    assert!(session.render(1, 99999, &mut [1; BLOCK_FRAMES]).is_err());
    assert_eq!(session.state(), AudioState::Streaming);
}

#[test]
fn t717_handshake_cannot_move_clock_backwards() {
    let mut session =
        AudioSession::with_entropy(Direction::Microphone, crate::credentials::tests::entropy);
    let grant = session
        .start(AudioProfile::new(Direction::Microphone), caps(), true, 100)
        .unwrap();
    assert!(session.connected(1, &grant.hello(), 99).is_err());
    session.connected(1, &grant.hello(), 100).unwrap();
    assert!(session
        .receive(1, &grant.encode(0, 0, &[0; BLOCK_FRAMES]).unwrap(), 99)
        .is_err());
}

#[test]
fn t717_poll_clock_must_not_regress_before_frame_arrives() {
    let (mut session, grant) = streaming(Direction::Microphone);
    assert_eq!(session.tick(200), None);
    assert!(session
        .receive(1, &grant.encode(0, 0, &[0; BLOCK_FRAMES]).unwrap(), 199)
        .is_err());
    assert_eq!(session.state(), AudioState::Stopping);
}

#[test]
fn t717_queue_stress_stays_bounded_with_extreme_samples_and_drift() {
    let mut queue = PcmQueue::new(AudioProfile::new(Direction::Speakers)).unwrap();
    let mut output = [0; MAX_SAMPLES];
    let mut seed = 717u64;
    for now in 0..4096 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        match seed % 5 {
            0 => queue.clear(),
            1 => {
                queue.render(now, &mut output).unwrap();
            }
            2 => {
                queue
                    .adjust_drift(999_000 + seed % 2001, 1_000_000)
                    .unwrap();
            }
            _ => queue
                .push(block(Direction::Speakers, (seed >> 32) as i16), now)
                .unwrap(),
        }
        assert!(queue.queued_frames() <= 20 * BLOCK_FRAMES);
    }
}

#[test]
fn t717_resampling_interpolates_across_block_boundary() {
    let mut queue = PcmQueue::new(AudioProfile::new(Direction::Microphone)).unwrap();
    for value in [0, 1000, 2000, 3000] {
        queue.push(block(Direction::Microphone, value), 0).unwrap();
    }
    queue.adjust_drift(1_001_000, 1_000_000).unwrap();
    let mut output = [0; BLOCK_FRAMES];
    queue.render(0, &mut output).unwrap();
    assert_eq!(output[0], 0);
    assert_eq!(output[478], 0);
    assert_eq!(output[479], 479);
    queue.render(0, &mut output).unwrap();
    assert_eq!(output[0], 1000);
    assert_eq!(output[479], 1959);
}

#[test]
fn t717_buffer_preferences_control_prefill_and_wire_golden_fields() {
    for ms in [20, 40, 200] {
        let mut profile = AudioProfile::new(Direction::Speakers);
        profile.buffer_ms = ms;
        profile.processing = Processing::Raw;
        profile.background = true;
        let grant = AudioGrant::new(
            profile,
            0x0102030405060708,
            crate::credentials::tests::entropy,
        )
        .unwrap();
        let hello = grant.hello();
        assert_eq!(&hello[..8], b"BLAUD001");
        assert_eq!(
            &hello[72..90],
            &[1, 2, 3, 4, 5, 6, 7, 8, 2, 2, 0, 0, 187, 128, 1, 224, 2, 1]
        );
        assert_eq!(&hello[90..], &ms.to_be_bytes());
        let mut queue = PcmQueue::new(profile).unwrap();
        for _ in 1..ms / 10 {
            queue.push(block(Direction::Speakers, 9), 0).unwrap();
        }
        assert!(queue.render(0, &mut [0; MAX_SAMPLES]).unwrap().silence);
        queue.push(block(Direction::Speakers, 9), 0).unwrap();
        let mut output = [0; MAX_SAMPLES];
        assert!(!queue.render(0, &mut output).unwrap().silence);
        assert_eq!(output, [9; MAX_SAMPLES]);
        let frame = grant
            .encode(0, 0x1112131415161718, &[i16::MIN; MAX_SAMPLES])
            .unwrap();
        assert_eq!(
            &frame[16..30],
            &[17, 18, 19, 20, 21, 22, 23, 24, 7, 128, 2, 0, 0, 128]
        );
    }
}

#[test]
fn t717_stale_failure_cannot_cancel_replacement_and_stop_cannot_restore_it() {
    let (mut session, grant) = streaming(Direction::Microphone);
    session.cancel(1, true);
    session.retired(1);
    assert_eq!(session.cancel(1, true), None);
    let next = session.start(grant.profile(), caps(), true, 100).unwrap();
    assert_eq!(session.cancel(1, true), None);
    session.connected(2, &next.hello(), 100).unwrap();
    assert_eq!(session.state(), AudioState::Streaming);
    assert_eq!(session.cancel(2, false), Some(2));
    session.retired(2);
    assert_eq!(session.state(), AudioState::Stopped);
    assert_eq!(session.tick(100000), None);
    assert!(session.connected(2, &next.hello(), 100).is_err());
}

struct FakeBackend {
    current: Option<AudioGrant>,
}

impl AudioBackend for FakeBackend {
    fn capabilities(&self) -> AudioCapabilities {
        caps()
    }
    fn start(&mut self, grant: AudioGrant) -> anyhow::Result<()> {
        anyhow::ensure!(self.current.is_none(), "native owner remains");
        self.current = Some(grant);
        Ok(())
    }
    fn stop(&mut self, generation: u64) {
        if self
            .current
            .as_ref()
            .is_some_and(|grant| grant.generation() == generation)
        {
            self.current = None;
        }
    }
}

#[test]
fn t717_fake_backend_obeys_owned_retirement_before_restart() {
    let mut backend = FakeBackend { current: None };
    let mut session =
        AudioSession::with_entropy(Direction::Speakers, crate::credentials::tests::entropy);
    let grant = session
        .start(
            AudioProfile::new(Direction::Speakers),
            backend.capabilities(),
            true,
            0,
        )
        .unwrap();
    backend.start(grant.clone()).unwrap();
    assert!(backend.start(grant.clone()).is_err());
    backend.stop(99);
    assert!(backend.current.is_some());
    let generation = session.stop(false).unwrap();
    assert!(session.start(grant.profile(), caps(), true, 0).is_err());
    backend.stop(generation);
    assert!(backend.current.is_none());
    assert!(session.retired(generation));
    let next = session.start(grant.profile(), caps(), true, 0).unwrap();
    backend.start(next).unwrap();
    backend.stop(generation);
    assert_eq!(backend.current.as_ref().unwrap().generation(), 2);
}

#[test]
fn t717_fractional_excess_drift_is_rejected_before_rounding() {
    let mut queue = PcmQueue::new(AudioProfile::new(Direction::Microphone)).unwrap();
    for frames in [998_999_999, 1_001_000_001] {
        queue.push(block(Direction::Microphone, 3), 0).unwrap();
        assert!(queue.adjust_drift(frames, 1_000_000_000).is_err());
        assert_eq!(queue.queued_frames(), 0);
    }
}

#[test]
fn t718_native_pcm_bounds_and_saved_preferences_are_passive() {
    use crate::FileConfig;
    for direction in [Direction::Microphone, Direction::Speakers] {
        let size = direction.channels() * BLOCK_FRAMES * 2;
        for length in [0, 1, size - 1, size, size + 1, 4096] {
            assert_eq!(
                PcmBlock::from_le_bytes(direction, &vec![0; length]).is_ok(),
                length == size
            );
        }
        let native = PcmBlock::from_le_bytes(direction, &vec![255; size]).unwrap();
        assert!(native.samples().iter().all(|sample| *sample == -1));
    }
    let baseline = FileConfig::default();
    let mut changed = baseline.clone();
    changed.audio.microphone.profile.processing = Processing::Raw;
    changed.audio.microphone.profile.buffer_ms = 200;
    changed.audio.microphone.profile.background = true;
    changed.audio.speakers.profile.processing = Processing::Raw;
    changed.audio.speakers.profile.buffer_ms = 20;
    changed.audio.speakers.profile.background = true;
    assert!(!changed.requires_restart_from(&baseline));
    let restored: FileConfig = toml::from_str(&toml::to_string(&changed).unwrap()).unwrap();
    assert_eq!(restored.audio, changed.audio);
    assert_eq!(AudioStatus::default().state, AudioState::Stopped);
}

#[test]
fn t719_speaker_defaults_upgrade_old_settings_without_starting() {
    let old = "[microphone.profile]\ndirection = 'microphone'\nprocessing = 'speech'\nbuffer_ms = 40\nbackground = false\n";
    let settings: AudioSettings = toml::from_str(old).unwrap();
    assert_eq!(settings.speakers, AudioOptions::new(Direction::Speakers));
    assert_eq!(
        settings.microphone,
        AudioOptions::new(Direction::Microphone)
    );
}

#[test]
fn t736_failed_entropy_never_admits_or_advances_a_session() {
    let direction = Direction::Microphone;
    let mut session = AudioSession::with_entropy(direction, |_| anyhow::bail!("entropy offline"));
    for time in 0..32 {
        assert!(session
            .start(AudioProfile::new(direction), caps(), true, time)
            .is_err());
        assert_eq!(session.state(), AudioState::Stopped);
        assert!(session.stop(false).is_none());
    }
    #[cfg(not(feature = "native-entropy"))]
    assert!(AudioSession::new(direction)
        .start(AudioProfile::new(direction), caps(), true, 0)
        .is_err());
}
