pub use blent_config::*;

/// Validate adapter capabilities before claiming capture resources or publishing settings.
pub fn validate_encoder_for_build(name: &str) -> anyhow::Result<()> {
    if name == "auto" {
        return Ok(());
    }
    let encoder = blent_config::encoding::find(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown encoder: {name}"))?;
    if cfg!(feature = "inproc-encoder") {
        encoder.validate_inproc()?;
    }
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
        for name in [
            "h264_vaapi",
            "h264_vaapi_baseline",
            "hevc_vaapi",
            "vaapih264enc",
        ] {
            assert_eq!(
                validate_encoder_for_build(name).is_ok(),
                !cfg!(feature = "inproc-encoder"),
                "T284: {name}"
            );
        }
        assert!(validate_encoder_for_build("unknown").is_err());
    }
}
