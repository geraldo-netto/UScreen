use super::super::{
    tests::{candidate, settings},
    Key,
};
use super::*;
use crate::{attachment::Attachment, capture::CaptureConfig};
use blent_config::adb::Transport;
use std::os::unix::fs::PermissionsExt;

pub(in crate::selection::worker) fn setup() -> (EncoderSettings, Candidate) {
    let mut snapshot = settings();
    let mut caps: blent_config::negotiation::DecoderCapabilities = serde_json::from_str(
        include_str!("../../../../../testdata/decoder-capabilities-v2.json"),
    )
    .unwrap();
    caps.software = Some("a".repeat(64));
    snapshot.decoders = Some(caps);
    let stream = blent_config::negotiation::StreamProfile {
        codec: "h264".into(),
        format: blent_config::negotiation::Profile {
            profile: "baseline".into(),
            level: 31,
            depth: 8,
        },
    };
    let mut row = candidate("libx264", true, 120.0, 1000);
    row.measurement.stream = Some(stream.clone());
    row.decoder = snapshot.decoders.as_ref().unwrap().choose(&stream, true);
    row.observation = Some(Observation {
        p50_us: 1000,
        p95_us: 2000,
        p99_us: 3000,
        ack_fps: 60.0,
        delivery_permille: 1000,
        startup_us: 5000,
        samples: 120,
    });
    (snapshot, row)
}
pub(in crate::selection::worker) fn store(path: PathBuf) -> Cache {
    Cache {
        path,
        fingerprint: "a".repeat(64),
        lease: None,
    }
}

#[test]
fn t480_atomic_private_roundtrip_ignores_interrupted_writes() {
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("space path/profile-cache.json"));
    let (snapshot, row) = setup();
    let now = now();
    assert!(cache
        .load(now, &snapshot, std::slice::from_ref(&row))
        .is_none());
    cache.save(now, &row).unwrap();
    let winner = cache
        .load(now, &snapshot, std::slice::from_ref(&row))
        .unwrap();
    assert!(winner.cached);
    assert_eq!(winner.decoder, row.decoder);
    assert_eq!(
        cache.path.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        cache.save(now, &winner).is_err(),
        "historical observation must not refresh its own age"
    );
    let mut incomplete = tempfile::NamedTempFile::new_in(cache.path.parent().unwrap()).unwrap();
    incomplete.write_all(b"{ interrupted").unwrap();
    drop(incomplete);
    assert!(cache.load(now, &snapshot, &[row]).is_some());
    cache.invalidate();
    assert!(!cache.path.exists());
    cache.invalidate();
}

#[test]
fn t480_age_context_future_time_and_manual_preference_invalidate() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = store(dir.path().join("cache"));
    let (mut snapshot, row) = setup();
    cache.save(100, &row).unwrap();
    for time in [0, 99, 100 + MAX_AGE + 1, u64::MAX] {
        assert!(cache
            .load(time, &snapshot, std::slice::from_ref(&row))
            .is_none());
    }
    assert!(cache
        .load(100 + MAX_AGE, &snapshot, std::slice::from_ref(&row))
        .is_some());
    cache.fingerprint = "different".into();
    cache.invalidate();
    assert!(cache.path.exists());
    assert!(cache
        .load(100, &snapshot, std::slice::from_ref(&row))
        .is_none());
    cache.fingerprint = "a".repeat(64);
    snapshot.encoder = "libx264".into();
    assert!(cache.load(100, &snapshot, &[row]).is_none());
}

#[test]
fn t480_unknown_software_caps_format_host_route_and_capture_invalidate() {
    let (snapshot, _) = setup();
    let base = CaptureConfig::default();
    let identity = ("tablet".into(), "usb");
    let original = context::fingerprint(&identity, "host", &snapshot, &base).unwrap();
    let mut migrated = snapshot.clone();
    migrated.decoder_epoch += 9;
    migrated.decoders.as_mut().unwrap().scope = Some("88".into());
    assert_eq!(
        context::fingerprint(&identity, "host", &migrated, &base).unwrap(),
        original
    );
    for changed in [("tablet".into(), "network"), ("replacement".into(), "usb")] {
        assert_ne!(
            context::fingerprint(&changed, "host", &snapshot, &base).unwrap(),
            original
        );
    }
    assert_ne!(
        context::fingerprint(&identity, "upgraded", &snapshot, &base).unwrap(),
        original
    );
    migrated.bitrate += 1;
    assert_ne!(
        context::fingerprint(&identity, "host", &migrated, &base).unwrap(),
        original
    );
    migrated.decoders.as_mut().unwrap().software = None;
    assert!(context::fingerprint(&identity, "host", &migrated, &base).is_none());
}

#[test]
fn t480_profile_must_pass_fresh_encoder_quality_capacity_and_decoder_support() {
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("cache"));
    let (mut snapshot, row) = setup();
    cache.save(100, &row).unwrap();
    let mut slow = row.clone();
    slow.measurement.fps = 59.0;
    assert!(cache.load(100, &snapshot, &[slow]).is_none());
    assert!(cache.load(100, &snapshot, &[]).is_none());
    let mut wrong = row.clone();
    wrong.measurement.stream.as_mut().unwrap().format.depth = 10;
    assert!(cache.load(100, &snapshot, &[wrong]).is_none());
    snapshot.decoders.as_mut().unwrap().details.clear();
    assert!(cache.load(100, &snapshot, &[row]).is_none());
}

#[test]
fn t480_decoder_alternative_hints_are_rechecked() {
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("cache"));
    let (snapshot, mut row) = setup();
    row.decoder.as_mut().unwrap().operating_rate = None;
    cache.save(100, &row).unwrap();
    assert!(cache
        .load(100, &snapshot, std::slice::from_ref(&row))
        .is_some());
    row.decoder.as_mut().unwrap().operating_rate = Some(180);
    cache.save(100, &row).unwrap();
    assert!(cache.load(100, &snapshot, &[row]).is_none());
}

#[test]
fn t480_malformed_oversized_and_symlinked_cache_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("cache"));
    let (snapshot, row) = setup();
    for input in [
        vec![],
        b"{broken".to_vec(),
        vec![b' '; MAX_BYTES as usize + 1],
    ] {
        std::fs::write(&cache.path, input).unwrap();
        assert!(cache
            .load(100, &snapshot, std::slice::from_ref(&row))
            .is_none());
    }
    std::fs::remove_file(&cache.path).unwrap();
    let other = dir.path().join("other");
    std::fs::write(&other, "unchanged").unwrap();
    std::os::unix::fs::symlink(&other, &cache.path).unwrap();
    assert!(cache.read().is_none());
    cache.save(100, &row).unwrap();
    assert_eq!(std::fs::read_to_string(other).unwrap(), "unchanged");
}

#[test]
fn t480_invalid_and_out_of_bounds_observations_never_load() {
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("cache"));
    let (snapshot, row) = setup();
    cache.save(100, &row).unwrap();
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache.path).unwrap()).unwrap();
    for field in [
        "samples",
        "p50_us",
        "p95_us",
        "p99_us",
        "delivery_permille",
        "startup_us",
        "ack_fps",
    ] {
        for value in [
            serde_json::json!(-1),
            serde_json::json!(u64::MAX),
            serde_json::json!("invalid"),
            serde_json::Value::Null,
        ] {
            let mut mutated = original.clone();
            mutated["observation"][field] = value;
            std::fs::write(&cache.path, serde_json::to_vec(&mutated).unwrap()).unwrap();
            assert!(
                cache
                    .load(100, &snapshot, std::slice::from_ref(&row))
                    .is_none(),
                "{field}"
            );
        }
    }
}

#[test]
fn t480_seeded_corruption_is_bounded_and_never_panics() {
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("cache"));
    let (snapshot, row) = setup();
    cache.save(100, &row).unwrap();
    let original = std::fs::read(&cache.path).unwrap();
    let mut seed = 480_u64;
    for _ in 0..512 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut data = original.clone();
        let index = (seed as usize) % data.len();
        data[index] ^= (seed >> 32) as u8;
        std::fs::write(&cache.path, data).unwrap();
        if let Some(loaded) = cache.load(100, &snapshot, std::slice::from_ref(&row)) {
            assert!(loaded.cached);
            assert!(observation_valid(loaded.observation.as_ref().unwrap()));
        }
    }
}

#[tokio::test]
async fn t480_proven_identity_and_attachment_retirement_bound_cache_use() {
    let (snapshot, row) = setup();
    let (tx, _rx) = tokio::sync::watch::channel(snapshot.clone());
    let attachment = Attachment::new(tx);
    let dir = tempfile::tempdir().unwrap();
    assert!(attachment.lease().profile_identity().is_none());
    attachment.begin_with_transport(Some("transport:serial".into()), Some(Transport::Usb));
    assert!(attachment.lease().profile_identity().is_none());
    attachment.begin_with_transport(Some("device:tablet".into()), Some(Transport::Usb));
    assert_eq!(
        attachment.lease().profile_identity().unwrap(),
        ("tablet".into(), "usb")
    );
    let mut cache = store(dir.path().join("cache"));
    cache.lease = Some(attachment.lease());
    cache.save(100, &row).unwrap();
    attachment.begin_with_transport(Some("device:tablet".into()), Some(Transport::Network));
    cache.retired().await;
    assert!(!cache.active());
    assert_eq!(attachment.lease().profile_identity().unwrap().1, "network");
    assert!(cache
        .load(100, &snapshot, std::slice::from_ref(&row))
        .is_none());
    assert!(cache.save(100, &row).is_err());
}

#[tokio::test]
async fn t480_cache_requires_opt_in_and_fresh_software_context() {
    let (snapshot, _) = setup();
    let (tx, _) = tokio::sync::watch::channel(snapshot.clone());
    let attachment = Attachment::new(tx);
    let mut config = CaptureConfig::default();
    assert!(Cache::open(&config, &snapshot, &attachment).await.is_none());
    config.profile_cache = true;
    assert!(Cache::open(&config, &snapshot, &attachment).await.is_none());
    attachment.begin_with_transport(Some("device:tablet".into()), Some(Transport::Usb));
    config.helper_path = std::env::current_exe().unwrap();
    let cache = Cache::open(&config, &snapshot, &attachment).await.unwrap();
    assert!(cache.active());
    assert_eq!(cache.fingerprint.len(), 64);
    assert_eq!(cache.path.file_name().unwrap(), "profile-cache.json");
}

#[tokio::test(start_paused = true)]
async fn t480_historical_choice_is_not_ranked_as_this_session_and_requires_fresh_acks() {
    let (snapshot, mut row) = setup();
    row.cached = true;
    let (tx, _) = tokio::sync::watch::channel(snapshot.clone());
    let key = Key::new(&snapshot);
    let chosen = super::super::choose(&tx, &key, "libx264", vec![row.clone()], |_| {
        std::future::ready(false)
    })
    .await;
    assert!(chosen.is_none());
    assert!(!tx.borrow().selection.as_ref().unwrap().verified);
    let chosen = super::super::choose(&tx, &key, "libx264", vec![row], |_| {
        std::future::ready(true)
    })
    .await;
    assert!(chosen.is_some());
    let selection = tx.borrow().selection.clone().unwrap();
    assert!(selection.verified);
    assert!(selection.reason.starts_with("Historical"));
    assert!(selection.reason.contains("not ranked this session"));
}

#[test]
fn t480_invalid_record_schema_quality_and_unknown_fields_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let cache = store(dir.path().join("cache"));
    let (snapshot, row) = setup();
    cache.save(100, &row).unwrap();
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache.path).unwrap()).unwrap();
    for (field, value) in [
        ("schema", serde_json::json!(0)),
        ("schema", serde_json::json!(u64::MAX)),
        ("quality_db", serde_json::json!(-1)),
        ("quality_db", serde_json::json!(201)),
        ("saved_at", serde_json::json!(-1)),
        ("unexpected", serde_json::json!(true)),
    ] {
        let mut mutated = original.clone();
        mutated[field] = value;
        std::fs::write(&cache.path, serde_json::to_vec(&mutated).unwrap()).unwrap();
        assert!(
            cache
                .load(100, &snapshot, std::slice::from_ref(&row))
                .is_none(),
            "{field}"
        );
    }
}
