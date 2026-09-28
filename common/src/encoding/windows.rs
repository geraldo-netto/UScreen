//! T685: Windows probe recipes. This catalog does not authorize live sessions.
use super::Profile;
use anyhow::{ensure, Result};

pub const CANDIDATES: [&str; 7] = [
    "h264_nvenc",
    "hevc_nvenc",
    "h264_amf",
    "hevc_amf",
    "h264_qsv",
    "hevc_qsv",
    "libx264",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Baseline,
    Main,
    High,
    Main10,
}
#[derive(Clone, Debug)]
pub struct Request {
    pub encoder: String,
    pub format: Format,
    pub fps: u32,
    pub bitrate: u32,
    pub quality: u32,
    /// Zero selects the conservative initial CPU candidate, one worker.
    /// Hardware encoders require zero: FFmpeg CPU threads are not GPU capacity.
    pub workers: u32,
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            CANDIDATES.contains(&self.encoder.as_str()),
            "Unknown Windows probe encoder"
        );
        ensure!(
            (crate::MIN_FPS..=crate::MAX_FPS).contains(&self.fps),
            "Invalid probe frame rate"
        );
        ensure!(
            (crate::MIN_BITRATE_KBPS..=crate::MAX_BITRATE_KBPS).contains(&self.bitrate),
            "Invalid probe bitrate"
        );
        ensure!(
            (crate::MIN_QUALITY..=crate::MAX_QUALITY).contains(&self.quality),
            "Invalid probe quality"
        );
        crate::encoder_workers::validate(self.workers)?;
        ensure!(
            self.encoder == "libx264" || self.workers == 0,
            "CPU worker choice requires libx264"
        );
        self.profile()?;
        Ok(())
    }
    pub fn codec(&self) -> crate::video::Codec {
        crate::video::Codec::from_encoder(&self.encoder)
    }
    fn profile(&self) -> Result<&'static str> {
        match (self.encoder.starts_with("hevc"), self.format) {
            (true, Format::Main) | (false, Format::Main) => Ok("main"),
            (true, Format::Main10) => Ok("main10"),
            (false, Format::Baseline) if self.encoder.ends_with("_amf") => {
                Ok("constrained_baseline")
            }
            (false, Format::Baseline) => Ok("baseline"),
            (false, Format::High) => Ok("high"),
            _ => anyhow::bail!("Profile is incompatible with encoder codec"),
        }
    }
    pub fn options(&self) -> Result<Vec<(String, String)>> {
        self.validate()?;
        let mut options = if self.encoder.ends_with("_amf") {
            self.amf()
        } else if self.encoder.ends_with("_qsv") {
            self.qsv()
        } else {
            Profile::new(&self.encoder, self.fps, self.bitrate, self.quality)?
                .with_workers(self.workers)?
                .cli_options(self.format == Format::Main10)
        };
        options.retain(|(key, _)| key != "-profile:v");
        options.push(("-profile:v".into(), self.profile()?.into()));
        options.push((
            "-pix_fmt".into(),
            if self.format == Format::Main10 {
                "p010le"
            } else {
                "nv12"
            }
            .into(),
        ));
        Ok(options)
    }
    fn amf(&self) -> Vec<(String, String)> {
        let mut options = pairs(&[
            ("-usage", "ultralowlatency"),
            ("-quality", "speed"),
            ("-rc", "cqp"),
            ("-async_depth", "1"),
            ("-bf", "0"),
            ("-forced_idr", "1"),
        ]);
        options.extend([
            ("-qp_i".into(), self.quality.to_string()),
            ("-qp_p".into(), self.quality.to_string()),
            ("-g".into(), self.fps.to_string()),
        ]);
        options
    }
    fn qsv(&self) -> Vec<(String, String)> {
        let mut options = pairs(&[
            ("-preset", "veryfast"),
            ("-async_depth", "1"),
            ("-bf", "0"),
            ("-forced_idr", "1"),
            ("-flags", "+qscale"),
        ]);
        options.extend([
            ("-global_quality".into(), self.quality.to_string()),
            ("-g".into(), self.fps.to_string()),
        ]);
        if self.encoder == "h264_qsv" {
            options.push(("-look_ahead".into(), "0".into()));
        }
        options
    }
    /// Isolated generated-frame probe. Adapters own program lookup, deadline,
    /// bounded reads, process lifetime and output verification.
    pub fn probe_arguments(&self, width: u32, height: u32, frames: u32) -> Result<Vec<String>> {
        self.validate()?;
        crate::raw_frame::Layout::new(width, height, 2)?;
        ensure!((2..=120).contains(&frames), "Invalid probe frame count");
        let mut args: Vec<String> = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
        ]
        .map(String::from)
        .into();
        args.push(format!("testsrc2=size={width}x{height}:rate={}", self.fps));
        args.extend([
            "-frames:v".into(),
            frames.to_string(),
            "-c:v".into(),
            self.encoder.clone(),
        ]);
        args.extend(
            self.options()?
                .into_iter()
                .flat_map(|(key, value)| [key, value]),
        );
        args.extend([
            "-flush_packets".into(),
            "1".into(),
            "-f".into(),
            self.codec().muxer().into(),
            "pipe:1".into(),
        ]);
        Ok(args)
    }
}
fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[cfg(test)]
mod tests;
