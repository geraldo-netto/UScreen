use super::*;

fn report() -> DecoderCapabilities {
    serde_json::from_str(include_str!(
        "../../../testdata/decoder-capabilities-v2.json"
    ))
    .unwrap()
}

#[test]
fn t484_decoder_receipt_matches_shared_android_vector_and_changes_with_hints() {
    let vector: serde_json::Value =
        serde_json::from_str(include_str!("../../../testdata/decoder-selection.json")).unwrap();
    let mut choice: DecoderChoice =
        serde_json::from_value(vector["decoder_selection"].clone()).unwrap();
    assert_eq!(choice.receipt(), vector["receipt"].as_str().unwrap());
    choice.operating_rate = None;
    assert_ne!(choice.receipt(), vector["receipt"].as_str().unwrap());
    choice.name = "10:vendor.avc".into();
    assert!(choice.receipt().starts_with("13:10:vendor.avc:"));
}

fn stream(profile: &str, depth: u8, level: u32) -> StreamProfile {
    StreamProfile {
        codec: "h264".into(),
        format: Profile {
            profile: profile.into(),
            depth,
            level,
        },
    }
}

#[test]
fn t478_shared_report_round_trips_and_intersects_exact_profiles() {
    let report = report();
    assert!(report.matches("7", 640, 480, 60));
    let choice = report
        .choose(&stream("constrained-baseline", 8, 31), true)
        .unwrap();
    assert_eq!(choice.name, "vendor.avc");
    assert!(
        !choice.low_latency,
        "T478: unknown feature became supported"
    );
    assert_eq!(choice.operating_rate, Some(120));
    assert_eq!(
        report,
        serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap()
    );
}

#[test]
fn t478_unknown_unsupported_depth_level_and_profile_never_authorize() {
    for required in [
        stream("main", 8, 31),
        stream("baseline", 10, 31),
        stream("baseline", 8, 62),
        stream("unknown", 8, 31),
    ] {
        assert!(report().choose(&required, true).is_none());
    }
    let mut unknown = report();
    unknown.details[0].profiles.clear();
    assert!(unknown.valid());
    assert!(unknown.choose(&stream("baseline", 8, 31), false).is_none());
}

#[test]
fn t478_scope_format_version_and_bounds_are_validated() {
    let good = report();
    assert!(!good.matches("8", 640, 480, 60));
    assert!(!good.matches("7", 640, 480, 30));
    let mut invalid = good.clone();
    invalid.protocol = 3;
    assert!(!invalid.valid());
    invalid = good.clone();
    invalid.details[0].name = "x".repeat(129);
    assert!(!invalid.valid());
    invalid = good.clone();
    invalid.details = vec![good.details[0].clone(); 17];
    assert!(!invalid.valid());
    invalid = good.clone();
    invalid.details[0].profiles = vec![good.details[0].profiles[0].clone(); 33];
    assert!(!invalid.valid());
}

#[test]
fn t478_old_peer_and_duplicate_identity_are_not_rich_support() {
    let mut report = report();
    report.protocol = 1;
    report.scope = None;
    report.details.clear();
    assert!(report.valid());
    assert!(report.choose(&stream("baseline", 8, 31), true).is_none());
    report = super::tests::report();
    report.details.push(report.details[0].clone());
    assert!(!report.valid());
}

#[test]
fn t478_avc_level_one_b_orders_between_one_and_one_one() {
    let mut report = report();
    report.details[0].profiles[0].level = 10;
    assert!(report.choose(&stream("baseline", 8, 9), false).is_none());
    report.details[0].profiles[0].level = 9;
    assert!(report.choose(&stream("baseline", 8, 10), false).is_some());
    assert!(report.choose(&stream("baseline", 8, 11), false).is_none());
}

#[test]
fn t480_software_fingerprint_is_bounded_and_optional_for_older_peers() {
    let base = serde_json::to_value(report()).unwrap();
    for value in [
        "".to_string(),
        "a".repeat(63),
        "g".repeat(64),
        "a".repeat(65),
    ] {
        let mut raw = base.clone();
        raw["software"] = value.into();
        assert!(!serde_json::from_value::<DecoderCapabilities>(raw)
            .unwrap()
            .valid());
    }
    let mut raw = base;
    raw["software"] = "a".repeat(64).into();
    assert!(serde_json::from_value::<DecoderCapabilities>(raw)
        .unwrap()
        .valid());
    assert!(report().valid());
}

#[test]
fn t668_report_dimensions_and_rate_are_independently_bounded() {
    for (field, lower, upper) in [("width", 2, 4096), ("height", 2, 4096), ("fps", 10, 90)] {
        for value in [0, lower - 1, lower, upper, upper + 1, u32::MAX] {
            let mut raw = serde_json::to_value(report()).unwrap();
            raw[field] = value.into();
            let decoded: DecoderCapabilities = serde_json::from_value(raw).unwrap();
            assert_eq!(
                decoded.valid(),
                (lower..=upper).contains(&value),
                "T668: {field}={value}"
            );
        }
    }
}

#[test]
fn t668_codec_families_and_hardware_subset_are_validated() {
    let mut good = report();
    good.codecs = ["h264", "hevc", "vp9", "av1"].map(str::to_owned).into();
    good.hardware = good.codecs.clone();
    assert!(good.valid());
    let mut too_many = good.clone();
    too_many.codecs.push("h264".into());
    assert!(!too_many.valid());
    too_many = good.clone();
    too_many.hardware.push("h264".into());
    assert!(!too_many.valid());
    for unknown in ["", "H264", "foreign", "h264\0", "vp9 "] {
        let mut invalid = good.clone();
        invalid.codecs[3] = unknown.into();
        invalid.hardware.clear();
        assert!(!invalid.valid(), "T668: unknown family {unknown:?}");
    }
    let mut missing_family = report();
    missing_family.hardware = vec!["av1".into()];
    assert!(!missing_family.valid());
}

#[test]
fn t668_legacy_reports_reject_either_rich_extension() {
    let mut legacy = report();
    legacy.protocol = 1;
    legacy.scope = None;
    legacy.details.clear();
    assert!(legacy.valid());
    let mut with_scope = legacy.clone();
    with_scope.scope = Some("7".into());
    assert!(!with_scope.valid());
    legacy.details = report().details;
    assert!(!legacy.valid());
    assert!(legacy.choices(&stream("baseline", 8, 31), false).is_empty());
}

#[test]
fn t668_invalid_reports_and_streams_cannot_offer_decoder_choices() {
    let mut invalid = report();
    invalid.width = 0;
    assert!(invalid
        .choices(&stream("baseline", 8, 31), false)
        .is_empty());
    for bad in [
        stream("baseline", 8, 0),
        stream("baseline", 10, 31),
        stream("foreign", 8, 31),
    ] {
        assert!(!bad.valid());
        assert!(report().choices(&bad, false).is_empty());
    }
}

#[test]
fn t668_hardware_choices_precede_software_without_losing_identity() {
    let mut caps = report();
    let mut software = caps.details[0].clone();
    software.name = "software.avc".into();
    software.hardware = Some(false);
    caps.details.insert(0, software);
    let choices = caps.choices(&stream("baseline", 8, 31), true);
    let names: Vec<_> = choices.iter().map(|choice| choice.name.as_str()).collect();
    assert_eq!(names, ["vendor.avc", "software.avc"]);
}

#[test]
fn t668_codec_profile_depth_and_level_contracts_are_independent() {
    for (codec, profile, depth, level) in [
        ("h264", "baseline", 8, 9),
        ("hevc", "main", 8, 10),
        ("hevc", "main10", 10, 62),
        ("vp9", "profile0", 8, 11),
        ("vp9", "profile2", 10, 62),
        ("av1", "main", 8, 20),
        ("av1", "main", 10, 73),
    ] {
        let valid = StreamProfile {
            codec: codec.into(),
            format: Profile {
                profile: profile.into(),
                depth,
                level,
            },
        };
        assert!(valid.valid(), "T668: supported {valid:?}");
        for invalid_level in [0, 1, 8, 74, u32::MAX] {
            let mut invalid = valid.clone();
            invalid.format.level = invalid_level;
            assert!(!invalid.valid(), "T668: invalid level {invalid:?}");
        }
        let mut invalid = valid.clone();
        invalid.format.depth = 12;
        assert!(!invalid.valid());
        invalid = valid;
        invalid.format.profile = "foreign".into();
        assert!(!invalid.valid());
    }
}
