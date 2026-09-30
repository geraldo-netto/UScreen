use super::*;

fn devices() -> Vec<Device> {
    vec![Device {
        adapter: blent_config::gpu::Adapter {
            id: "stable-one".into(),
            label: "First".into(),
            backend: blent_config::encoding::Backend::Vaapi,
            accessible: true,
        },
        node: "/native/renderD129".into(),
    }]
}

#[test]
fn t727_resolves_native_device_and_rejects_missing_denied_or_unsupported_backends() {
    let mut native = devices();
    for (request, encoder, supported, expected) in [
        ("", "h264_nvenc", true, "h264_nvenc"),
        (
            "stable-one",
            "h264_vaapi_baseline",
            true,
            "h264_vaapi_baseline",
        ),
        ("stable-one", "auto", true, "auto"),
        ("stable-one", "h264_nvenc", true, "libx264"),
        ("stable-one", "h264_vaapi", false, "libx264"),
        ("missing", "h264_vaapi", true, "libx264"),
        ("software", "h264_vaapi", true, "libx264"),
    ] {
        let mut config = CaptureConfig {
            encoder: encoder.into(),
            ..Default::default()
        };
        resolve(&mut config, request, &native, supported);
        assert_eq!(config.encoder, expected);
        if request == "stable-one" {
            assert_eq!(config.vaapi_device, "/native/renderD129");
        }
    }
    native[0].adapter.accessible = false;
    let mut config = CaptureConfig::default();
    resolve(&mut config, "stable-one", &native, true);
    assert_eq!(config.encoder, "libx264");
    assert_eq!(config.gpu_policy, Policy::Software);
}

#[tokio::test]
async fn t727_startup_preserves_legacy_and_reports_software_policy() {
    let mut config = CaptureConfig::default();
    select_gpu(&mut config, "").await;
    assert_eq!(config.gpu_policy, Policy::Automatic);
    select_gpu(&mut config, "software").await;
    assert_eq!(config.encoder, "libx264");
    assert_eq!(config.gpu_policy, Policy::Software);
}

#[cfg(not(feature = "inproc-encoder"))]
#[tokio::test]
async fn t727_failed_native_codec_probe_falls_back_and_auto_keeps_its_probe_matrix() {
    for (encoder, works, expected, called) in [
        ("h264_vaapi", true, "h264_vaapi", true),
        ("av1_vaapi", false, "libx264", true),
        ("auto", false, "auto", false),
        ("libx264", false, "libx264", false),
    ] {
        let count = std::cell::Cell::new(0);
        let mut config = CaptureConfig {
            encoder: encoder.into(),
            ..Default::default()
        };
        validate_requested(&mut config, |observed| {
            assert_eq!(observed.encoder, encoder);
            count.set(count.get() + 1);
            std::future::ready(works)
        })
        .await;
        assert_eq!(config.encoder, expected);
        assert_eq!(count.get(), usize::from(called));
    }
}

#[test]
fn t727_unsupported_build_uses_software_without_rejecting_the_saved_gpu_codec() {
    assert_eq!(initial_encoder("h264_vaapi", ""), "h264_vaapi");
    assert_eq!(
        initial_encoder("h264_vaapi", "stable-one"),
        if cfg!(feature = "inproc-encoder") {
            "libx264"
        } else {
            "h264_vaapi"
        }
    );
}

#[cfg(not(feature = "inproc-encoder"))]
#[tokio::test]
async fn t727_native_adapters_use_resolved_device_for_isolated_encoder_probe() {
    for device in gpu::discover()
        .into_iter()
        .filter(|device| device.adapter.accessible)
    {
        let mut config = CaptureConfig {
            encoder: "h264_vaapi_baseline".into(),
            width: 128,
            height: 128,
            ..Default::default()
        };
        select_gpu(&mut config, &device.adapter.id).await;
        assert_eq!(config.vaapi_device, device.node.to_string_lossy());
        assert!(matches!(
            config.encoder.as_str(),
            "h264_vaapi_baseline" | "libx264"
        ));
        eprintln!(
            "T727 native {} -> {} ({})",
            device.adapter.id, config.encoder, config.vaapi_device
        );
    }
}
