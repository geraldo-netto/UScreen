use super::*;

fn obu(kind: u8, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() < 128);
    [vec![(kind << 3) | 2, payload.len() as u8], payload.to_vec()].concat()
}
fn sequence() -> Vec<u8> {
    obu(1, &[0])
}
fn unit(prefix: u8) -> Vec<u8> {
    [obu(2, &[]), sequence(), obu(6, &[prefix, 0])].concat()
}

#[test]
fn t433_av1_keyframes_repeat_current_sequence_after_delimiter() {
    let mut parser = Av1State::default();
    assert_eq!(parser.prepare(&mut unit(0x10)).unwrap(), Some(true));
    for prefix in [0, 0x30, 0x50, 0x70, 0x80, 0x90] {
        assert_eq!(parser.prepare(&mut unit(prefix)).unwrap(), Some(false));
    }
    let mut join = [obu(2, &[]), obu(6, &[0x10, 0])].concat();
    assert_eq!(parser.prepare(&mut join).unwrap(), Some(true));
    assert_eq!(join, unit(0x10));
    // A recreated parser must decode a later prepared keyframe independently.
    assert_eq!(Av1State::default().prepare(&mut join).unwrap(), Some(true));
    let changed = obu(1, &[0, 88]);
    assert_eq!(parser.prepare(&mut changed.clone()).unwrap(), None);
    let mut key = obu(6, &[0x10]);
    parser.prepare(&mut key).unwrap();
    assert!(key.starts_with(&changed));
}

#[test]
fn t433_av1_split_frame_headers_and_still_picture() {
    let mut split = [sequence(), obu(3, &[0x10]), obu(4, &[1, 2])].concat();
    assert_eq!(Av1State::default().prepare(&mut split).unwrap(), Some(true));
    let mut still = [obu(1, &[0x18]), obu(6, &[0])].concat();
    assert_eq!(Av1State::default().prepare(&mut still).unwrap(), Some(true));
}

#[test]
fn t433_av1_rejects_invalid_bounds_profiles_and_ambiguous_units() {
    let bad = [
        vec![0x80],
        vec![0x08, 0],
        vec![0x0b, 0],
        vec![0x0a, 0x80],
        vec![0x0a, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        vec![0x0a, 2, 0],
        vec![0x0e, 8, 1, 0],
        obu(1, &[]),
        obu(1, &[0x20]),
        obu(1, &[8]),
        obu(6, &[0x10]),
        [sequence(), obu(6, &[])].concat(),
        [unit(0x10), obu(6, &[0x10])].concat(),
        [unit(0x10), sequence()].concat(),
        [unit(0x10), obu(2, &[])].concat(),
        [obu(2, &[0]), unit(0x10)].concat(),
    ];
    for mut data in bad {
        assert!(
            Av1State::default().prepare(&mut data).is_err(),
            "T433: {data:?}"
        );
    }
    let mut many = obu(15, &[]).repeat(4097);
    assert!(Av1State::default().prepare(&mut many).is_err());
    let mut too_large = vec![0x0a, 0x80, 0x80, 4]; // 65536 payload + OBU header
    too_large.resize(65540, 0);
    assert!(Av1State::default().prepare(&mut too_large).is_err());
}
