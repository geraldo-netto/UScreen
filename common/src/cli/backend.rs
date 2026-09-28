//! Capability admission precedes side effects; adapters apply supported settings.
use super::Cli;
use crate::{platform::Capabilities, FileConfig};
use anyhow::{ensure, Result};

impl Cli {
    pub fn validate_backend_options(
        &self,
        capabilities: Capabilities,
        direct_start: bool,
    ) -> Result<()> {
        let unavailable: Vec<_> = self
            .backend_options(capabilities)
            .into_iter()
            .filter_map(|(name, present, supported)| (present && !supported).then_some(name))
            .collect();
        ensure!(
            unavailable.is_empty(),
            "Options unavailable on this backend: {}",
            unavailable.join(", ")
        );
        ensure!(
            direct_start || (self.video_port.is_none() && self.input_port.is_none()),
            "Connection overrides require a direct daemon start"
        );
        Ok(())
    }

    fn backend_options(&self, capabilities: Capabilities) -> [(&'static str, bool, bool); 12] {
        let display = capabilities.display;
        [
            ("--edid", self.edid.is_some(), capabilities.system_setup),
            ("--helper", self.helper.is_some(), capabilities.system_setup),
            ("--encoder", self.encoder.is_some(), display),
            ("--fps", self.fps.is_some(), display),
            ("--bitrate", self.bitrate.is_some(), display),
            ("--width", self.width.is_some(), display),
            ("--height", self.height.is_some(), display),
            ("--quality", self.quality.is_some(), display),
            ("--stream-scale", self.stream_scale.is_some(), display),
            (
                "--conversion-threads",
                self.conversion_threads.is_some(),
                capabilities.conversion_pool,
            ),
            ("--encoder-workers", self.encoder_workers.is_some(), display),
            ("--pen-only", self.pen_only, capabilities.input),
        ]
    }

    pub fn connection_settings(&self, mut settings: FileConfig) -> Result<FileConfig> {
        settings.video_port = self.video_port.unwrap_or(settings.video_port);
        settings.input_port = self.input_port.unwrap_or(settings.input_port);
        crate::slot_ports(
            settings.video_port,
            settings.input_port,
            settings.max_tablets,
        )?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn capabilities(supported: bool) -> Capabilities {
        Capabilities {
            camera: supported,
            daemon: true,
            display: supported,
            input: supported,
            system_setup: supported,
            autostart: true,
            pipe_capacity: supported,
            conversion_pool: supported,
        }
    }

    #[test]
    fn t694_capability_admission_is_explicit_and_preserves_supported_arguments() {
        for args in [
            vec!["--edid", "x"],
            vec!["--helper", "x"],
            vec!["--encoder", "libx264"],
            vec!["--fps", "30"],
            vec!["--bitrate", "10000"],
            vec!["--width", "1280"],
            vec!["--height", "720"],
            vec!["--quality", "20"],
            vec!["--stream-scale", "2"],
            vec!["--conversion-threads", "auto"],
            vec!["--encoder-workers", "128"],
            vec!["--pen-only"],
        ] {
            let cli =
                Cli::try_parse_from(std::iter::once("blent").chain(args.iter().copied())).unwrap();
            let error = cli
                .validate_backend_options(capabilities(false), true)
                .unwrap_err()
                .to_string();
            assert!(error.contains(args[0]), "T694: {error}");
            cli.validate_backend_options(capabilities(true), true)
                .unwrap();
        }
        Cli::try_parse_from(["blent"])
            .unwrap()
            .validate_backend_options(capabilities(false), false)
            .unwrap();
    }

    #[test]
    fn t694_connection_overrides_preserve_saved_values_and_bound_slot_ranges() {
        for value in [0, 1, 8890, 65530, 65534, 65535] {
            for port in ["--video-port", "--input-port"] {
                let cli = Cli::try_parse_from(["blent", port, &value.to_string()]).unwrap();
                cli.validate_backend_options(capabilities(false), true)
                    .unwrap();
                assert!(cli
                    .validate_backend_options(capabilities(false), false)
                    .is_err());
                let saved = FileConfig {
                    max_tablets: 4,
                    fps: 30,
                    ..Default::default()
                };
                let expected = if port == "--video-port" {
                    (value, saved.input_port)
                } else {
                    (saved.video_port, value)
                };
                let valid = crate::slot_ports(expected.0, expected.1, saved.max_tablets).is_ok();
                let result = cli.connection_settings(saved);
                assert_eq!(result.is_ok(), valid, "T694: {port} {value}");
                if let Ok(settings) = result {
                    assert_eq!((settings.video_port, settings.input_port), expected);
                    assert_eq!(settings.fps, 30);
                }
            }
        }
        let saved = FileConfig::default();
        assert_eq!(
            Cli::try_parse_from(["blent"])
                .unwrap()
                .connection_settings(saved.clone())
                .unwrap(),
            saved
        );
    }
}
