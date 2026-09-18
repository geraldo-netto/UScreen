//! Encoder policy shared by the FFmpeg CLI and optional libavcodec adapters.
//! No FFmpeg dependency: adapters translate these values through stock public APIs.
use anyhow::{Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Nvenc,
    Vaapi,
    X264,
}

#[derive(Clone, Copy, Debug)]
pub struct Encoder {
    pub name: &'static str,
    pub label: &'static str,
    pub backend: Backend,
    pub hevc: bool,
}

pub const ENCODERS: [Encoder; 6] = [
    Encoder {
        name: "h264_nvenc",
        label: "NVIDIA H.264 (NVENC)",
        backend: Backend::Nvenc,
        hevc: false,
    },
    Encoder {
        name: "hevc_nvenc",
        label: "NVIDIA HEVC (NVENC)",
        backend: Backend::Nvenc,
        hevc: true,
    },
    Encoder {
        name: "h264_vaapi",
        label: "AMD / Intel H.264 (VAAPI)",
        backend: Backend::Vaapi,
        hevc: false,
    },
    Encoder {
        name: "h264_vaapi_baseline",
        label: "AMD / Intel H.264 low latency (VAAPI)",
        backend: Backend::Vaapi,
        hevc: false,
    },
    Encoder {
        name: "hevc_vaapi",
        label: "AMD / Intel HEVC (VAAPI)",
        backend: Backend::Vaapi,
        hevc: true,
    },
    Encoder {
        name: "libx264",
        label: "Software (libx264)",
        backend: Backend::X264,
        hevc: false,
    },
];

pub fn canonical_name(name: &str) -> &str {
    match name {
        "vaapih264enc" => "h264_vaapi",
        _ => name,
    }
}

/// A selectable profile is not necessarily a distinct FFmpeg encoder.
pub fn ffmpeg_name(name: &str) -> &str {
    match canonical_name(name) {
        "h264_vaapi_baseline" => "h264_vaapi",
        other => other,
    }
}

pub fn find(name: &str) -> Option<&'static Encoder> {
    let canonical = canonical_name(name);
    ENCODERS.iter().find(|encoder| encoder.name == canonical)
}

pub const INPROC_VAAPI_UNSUPPORTED: &str = "VAAPI is unavailable in this in-process build: UScreen does not create a hardware-frames context or use vaapi_device here. Build without --features inproc-encoder to use VAAPI, or select libx264/NVENC.";

impl Encoder {
    pub fn validate_inproc(&self) -> Result<()> {
        anyhow::ensure!(self.backend != Backend::Vaapi, INPROC_VAAPI_UNSUPPORTED);
        Ok(())
    }
}

pub struct Profile {
    pub encoder: &'static Encoder,
    /// A nominal second of captured frames. Each adapter has its own idle/join IDR mechanism.
    pub gop: u32,
    pub max_rate_bps: u64,
    bitrate_kbps: u32,
    quality: u32,
    buffer_kbits: Option<u32>,
}

impl Profile {
    pub fn new(name: &str, fps: u32, bitrate_kbps: u32, quality: u32) -> Result<Self> {
        let encoder = find(name).with_context(|| format!("Unknown encoder: {name}"))?;
        let gop = fps.max(1);
        let buffer_kbits = match encoder.backend {
            Backend::Nvenc => Some((bitrate_kbps / gop).max(200)),
            Backend::X264 => Some((bitrate_kbps * 2 / gop).max(200)),
            Backend::Vaapi => None,
        };
        Ok(Self {
            encoder,
            gop,
            max_rate_bps: u64::from(bitrate_kbps) * 1000,
            bitrate_kbps,
            quality,
            buffer_kbits,
        })
    }

    fn codec_options(&self) -> Vec<(&'static str, String)> {
        let (static_options, quality_key): (&[(&str, &str)], _) = match self.encoder.backend {
            Backend::Nvenc => (
                &[
                    ("preset", "p1"),
                    ("tune", "ull"),
                    ("zerolatency", "1"),
                    ("delay", "0"),
                    ("rc", "vbr"),
                    ("multipass", "0"),
                    ("rc-lookahead", "0"),
                    ("forced-idr", "1"),
                ],
                "cq",
            ),
            Backend::Vaapi => (&[("rc_mode", "CQP")], "qp"),
            Backend::X264 => (&[("preset", "ultrafast"), ("tune", "zerolatency")], "crf"),
        };
        let mut options = static_options
            .iter()
            .map(|&(key, value)| (key, value.into()))
            .collect::<Vec<_>>();
        options.push((quality_key, self.quality.to_string()));
        options
    }

    /// CLI syntax and explicit CLI-only controls. The caller limits ten_bit to HEVC.
    pub fn cli_options(&self, ten_bit: bool) -> Vec<(String, String)> {
        let mut options = self
            .codec_options()
            .into_iter()
            .map(|(key, value)| (format!("-{key}"), value))
            .collect::<Vec<_>>();
        // VAAPI CQP retains this existing option but does not enforce a bitrate ceiling (T259).
        options.push(("-maxrate".into(), format!("{}k", self.bitrate_kbps)));
        options.push(("-g".into(), self.gop.to_string()));
        if let Some(buffer) = self.buffer_kbits {
            options.push(("-bufsize".into(), format!("{buffer}k")));
        }
        self.cli_controls(ten_bit, &mut options);
        options
    }

    fn cli_controls(&self, ten_bit: bool, options: &mut Vec<(String, String)>) {
        let controls: &[(&str, &str)] = match self.encoder.backend {
            Backend::Nvenc => &[("-bf", "0"), ("-b:v", "0")],
            // T400: request processing depth one when the CLI capability probe
            // confirms support; avoids a measured extra input interval.
            Backend::Vaapi => &[("-bf", "0"), ("-idr_interval", "0"), ("-async_depth", "1")],
            Backend::X264 => &[("-x264-params", "scenecut=0")],
        };
        options.extend(
            controls
                .iter()
                .map(|&(key, value)| (key.into(), value.into())),
        );
        if self.encoder.name == "h264_vaapi_baseline" {
            options.push(("-profile:v".into(), "constrained_baseline".into()));
            options.push(("-coder".into(), "cavlc".into()));
        }
        if self.encoder.backend == Backend::Nvenc && ten_bit {
            options.push(("-profile:v".into(), "main10".into()));
        }
    }

    /// Libavcodec receives rate/GOP/B-frame/color fields through its typed context.
    /// Its input remains 8-bit NV12; VAAPI has no hardware-frame adapter.
    pub fn inproc_options(&self) -> Result<Vec<(&'static str, String)>> {
        self.encoder.validate_inproc()?;
        let mut options = self.codec_options();
        if let Some(buffer) = self.buffer_kbits {
            options.push(("bufsize", (u64::from(buffer) * 1000).to_string()));
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t400_low_latency_vaapi_is_distinct_from_stock_encoder_name() {
        let profile = Profile::new("h264_vaapi_baseline", 60, 20000, 18)
            .expect("T400: explicit low-latency profile must be selectable");
        assert_eq!(profile.encoder.backend, Backend::Vaapi);
        assert!(!profile.encoder.hevc);
        assert!(profile.encoder.validate_inproc().is_err());
        assert_eq!(
            crate::ffmpeg_encoder_name("h264_vaapi_baseline"),
            "h264_vaapi"
        );
        let options = profile.cli_options(false);
        assert!(options.contains(&("-profile:v".into(), "constrained_baseline".into())));
        assert!(options.contains(&("-coder".into(), "cavlc".into())));
        assert!(!Profile::new("h264_vaapi", 60, 20000, 18)
            .unwrap()
            .cli_options(false)
            .iter()
            .any(|(key, _)| key == "-profile:v"));
    }

    #[test]
    fn t373_registry_preserves_aliases_and_adapter_capabilities() {
        assert_eq!(find("vaapih264enc").unwrap().name, "h264_vaapi");
        assert!(find("unknown").is_none());
        for encoder in ENCODERS {
            assert_eq!(encoder.hevc, encoder.name.starts_with("hevc"));
            assert_eq!(
                encoder.validate_inproc().is_ok(),
                encoder.backend != Backend::Vaapi
            );
            for fps in [0, 10, 90] {
                let profile = Profile::new(encoder.name, fps, 60000, 32).unwrap();
                assert_eq!(profile.gop, fps.max(1));
                assert_eq!(profile.max_rate_bps, 60000000);
                assert_eq!(
                    profile.inproc_options().is_ok(),
                    encoder.backend != Backend::Vaapi
                );
            }
        }
    }
}
