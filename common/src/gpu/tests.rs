use super::*;

#[test]
fn t727_policy_preserves_automatic_and_bounds_invalid_or_stale_choices() {
    let mut catalog = Catalog {
        supported: true,
        adapters: vec![Adapter {
            id: "stable-one".into(),
            label: "First GPU".into(),
            backend: Backend::Vaapi,
            accessible: true,
        }],
    };
    assert_eq!(catalog.requested(""), "Automatic / existing device setting");
    assert_eq!(catalog.requested("software"), "CPU (software encoders)");
    assert_eq!(catalog.requested("stable-one"), "First GPU");
    assert_eq!(catalog.policy(""), Policy::Automatic);
    assert_eq!(catalog.policy("stable-one"), Policy::Pinned(Backend::Vaapi));
    for length in 0..512 {
        let invalid = format!("../{}", "界".repeat(length));
        assert_eq!(catalog.policy(&invalid), Policy::Software);
        assert!(catalog.requested(&invalid).chars().count() <= 124);
    }
    for encoder in crate::encoding::ENCODERS {
        let software = matches!(encoder.backend, Backend::X264 | Backend::Vpx | Backend::Aom);
        for policy in [
            Policy::Automatic,
            Policy::Software,
            Policy::Pinned(Backend::Vaapi),
        ] {
            let allowed = policy == Policy::Automatic
                || software
                || policy == Policy::Pinned(encoder.backend);
            assert_eq!(policy.allows(encoder.name), allowed);
            assert_eq!(
                policy.encoder(encoder.name),
                if allowed { encoder.name } else { "libx264" }
            );
        }
    }
    assert!(Policy::Software.allows("auto"));
    assert!(!Policy::Software.allows("invented"));
    catalog.adapters[0].accessible = false;
    assert_eq!(catalog.policy("stable-one"), Policy::Software);
    catalog.adapters[0].accessible = true;
    catalog.supported = false;
    assert_eq!(catalog.policy("stable-one"), Policy::Software);
    assert_eq!(Catalog::default().policy("missing"), Policy::Software);
}

#[test]
fn t727_config_roundtrip_merge_and_restart_preserve_legacy_device() {
    let original: crate::FileConfig =
        toml::from_str("vaapi_device = '/legacy/render-node'\n").unwrap();
    assert_eq!(original.encoding_gpu, "");
    let mut edited = original.clone();
    edited.encoding_gpu = "vaapi:pci-0000:03:00.0-render".into();
    assert!(edited.requires_restart_from(&original));
    let restored: crate::FileConfig = toml::from_str(&toml::to_string(&edited).unwrap()).unwrap();
    assert_eq!(restored, edited);
    let mut latest = original.clone();
    latest.quality = 24;
    let merged = edited.merge_edits(&original, latest).unwrap();
    assert_eq!(merged.encoding_gpu, edited.encoding_gpu);
    assert_eq!(merged.vaapi_device, "/legacy/render-node");
    assert_eq!(merged.quality, 24);
    assert_eq!(
        serde_json::from_str::<Policy>(
            &serde_json::to_string(&Policy::Pinned(Backend::Vaapi)).unwrap()
        )
        .unwrap(),
        Policy::Pinned(Backend::Vaapi)
    );
}
