pub use uscreen_config::*;

/// Validate adapter capabilities before claiming capture resources or publishing settings.
pub fn validate_encoder_for_build(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(supported_encoder(name), "Unknown encoder: {name}");
    #[cfg(feature = "inproc-encoder")]
    anyhow::ensure!(
        !matches!(ffmpeg_encoder_name(name), "h264_vaapi" | "hevc_vaapi"),
        "VAAPI is unavailable in this in-process build: UScreen does not create a \
         hardware-frames context or use vaapi_device here. Build without --features \
         inproc-encoder to use VAAPI, or select libx264/NVENC."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t284_encoder_capabilities_preserve_default_vaapi_and_supported_alternatives() {
        for name in ["h264_nvenc", "hevc_nvenc", "libx264"] {
            assert!(validate_encoder_for_build(name).is_ok(), "T284: {name}");
        }
        for name in ["h264_vaapi", "hevc_vaapi", "vaapih264enc"] {
            assert_eq!(
                validate_encoder_for_build(name).is_ok(),
                !cfg!(feature = "inproc-encoder"),
                "T284: {name}"
            );
        }
        assert!(validate_encoder_for_build("unknown").is_err());
    }
}
