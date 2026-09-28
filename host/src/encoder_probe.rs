//! T530: isolated stock-FFmpeg inventory/probes, independent of display capture.
use anyhow::{ensure, Context, Result};
use blent_config::{
    commands::OwnedChild,
    encoding::windows::{Format, Request, CANDIDATES},
    negotiation::{DecoderCapabilities, DecoderChoice, StreamProfile},
};
use std::{
    ffi::OsString,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::process::Command;

const DEADLINE: Duration = Duration::from_secs(8);
const TOTAL: Duration = Duration::from_secs(45);
const FRAMES: usize = 65;

/// OS-specific executable resolution is injected; process ownership uses the
/// existing native process-group/Windows-job adapter.
pub struct Programs {
    pub ffmpeg: OsString,
    pub ffprobe: OsString,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub epoch: u64,
    pub decoders: DecoderCapabilities,
    pub encoder: Option<String>,
    pub bitrate: u32,
    pub quality: u32,
    pub workers: u32,
    pub ten_bit: bool,
}
#[derive(Debug)]
pub struct Measurement {
    pub first_us: u64,
    pub p95_us: u64,
    pub fps: f64,
    pub workers_requested: u32,
    pub workers_effective: Option<u32>,
    pub stream: StreamProfile,
}
#[derive(Debug)]
pub struct Attempt {
    pub encoder: String,
    pub advertised: bool,
    /// At least one framed encoded packet observed, not just process startup.
    pub initialized: bool,
    pub packets: usize,
    pub measurement: Option<Measurement>,
    pub decoder: Option<DecoderChoice>,
    pub error: Option<String>,
}
impl Attempt {
    fn eligible(&self) -> bool {
        self.decoder.is_some() && self.measurement.is_some() && self.error.is_none()
    }
}
pub struct Selection {
    pub key: Key,
    pub attempts: Vec<Attempt>,
}
impl Selection {
    pub fn best(&self, current: &Key) -> Option<&Attempt> {
        if &self.key != current {
            return None;
        }
        self.attempts
            .iter()
            .filter(|a| a.eligible())
            .min_by_key(|a| {
                let m = a
                    .measurement
                    .as_ref()
                    .expect("matched decoder requires measurement");
                let hardware = current.decoders.details.iter().any(|d| {
                    a.decoder.as_ref().is_some_and(|c| {
                        d.name == c.name && d.codec == c.stream.codec && d.hardware == Some(true)
                    })
                });
                (
                    blent_config::encoding::rank(
                        m.fps,
                        hardware,
                        m.p95_us,
                        m.first_us,
                        current.decoders.fps,
                    ),
                    &a.encoder,
                )
            })
    }
}
impl Programs {
    pub async fn inventory(&self) -> Result<Vec<String>> {
        let bytes = bounded_output(&self.ffmpeg, &["-hide_banner", "-encoders"], 1_048_576).await?;
        inventory(&bytes)
    }
    pub async fn select(&self, key: Key) -> Result<Selection> {
        validate(&key)?;
        let advertised = self.inventory().await?;
        let mut selection = Selection {
            key,
            attempts: Vec::new(),
        };
        let deadline = tokio::time::Instant::now() + TOTAL;
        for request in requests(&selection.key)? {
            let present = advertised.contains(&request.encoder);
            let mut attempt = Attempt {
                encoder: request.encoder.clone(),
                advertised: present,
                initialized: false,
                packets: 0,
                measurement: None,
                decoder: None,
                error: None,
            };
            if present {
                match tokio::time::timeout_at(
                    deadline,
                    self.measure(&selection.key, &request, &mut attempt),
                )
                .await
                {
                    Ok(Ok(())) => (),
                    Ok(Err(error)) => attempt.error = Some(error.to_string()),
                    Err(_) => attempt.error = Some("Probe selection deadline".into()),
                }
            } else {
                attempt.error = Some("Encoder not advertised".into());
            }
            selection.attempts.push(attempt);
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }
        Ok(selection)
    }
    async fn measure(&self, key: &Key, request: &Request, attempt: &mut Attempt) -> Result<()> {
        let operation = async {
            let (times, sample) = self.encode(key, request, attempt).await?;
            let stream = crate::probe_format::inspect_bytes(
                &self.ffprobe,
                request.codec(),
                key.decoders.width,
                key.decoders.height,
                &sample,
            )
            .await?;
            let mut measured = summarize(&times, stream)?;
            measured.workers_requested = request.workers;
            if request.encoder == "libx264" {
                measured.workers_effective = blent_config::encoder_workers::effective_x264(&sample);
            }
            let expected_depth = if request.format == Format::Main10 {
                10
            } else {
                8
            };
            ensure!(
                measured.stream.format.depth == expected_depth,
                "Unexpected encoder depth"
            );
            attempt.decoder = key.decoders.choose(&measured.stream, true);
            attempt.measurement = Some(measured);
            ensure!(attempt.decoder.is_some(), "No compatible tablet decoder");
            Ok(())
        };
        tokio::time::timeout(DEADLINE, operation)
            .await
            .context("Encoder probe deadline")?
    }
    async fn encode(
        &self,
        key: &Key,
        request: &Request,
        attempt: &mut Attempt,
    ) -> Result<(Vec<u64>, Vec<u8>)> {
        let mut args =
            request.probe_arguments(key.decoders.width, key.decoders.height, FRAMES as u32)?;
        args.truncate(args.len() - 3);
        args.extend(
            [
                "-map",
                "0:v:0",
                "-f",
                "tee",
                crate::framed_annex_b::TEE_OUTPUT,
            ]
            .map(String::from),
        );
        let mut command = Command::new(&self.ffmpeg);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let started = Instant::now();
        let mut child = OwnedChild::spawn(&mut command)?;
        let mut reader =
            tokio::io::BufReader::new(child.take_stdout().context("Missing probe output")?);
        let mut parser =
            crate::framed_annex_b::FramedAnnexB::new(request.codec(), Default::default());
        let mut times = Vec::new();
        let mut sample = None;
        loop {
            let (count, packets) = parser.read_from(&mut reader).await?;
            ensure!(
                times.len() + packets.len() <= FRAMES,
                "Excess probe packets"
            );
            if !packets.is_empty() {
                ensure!(
                    parser.timestamp_us().is_some(),
                    "Missing probe packet timestamp"
                );
                attempt.initialized = true;
            }
            times.extend(packets.iter().map(|_| started.elapsed().as_micros() as u64));
            attempt.packets = times.len();
            if sample.is_none() {
                sample = packets.into_iter().find(|p| p.is_idr);
            }
            if count == 0 {
                break;
            }
        }
        ensure!(
            child.finish(DEADLINE).await?.success(),
            "Encoder initialization or execution failed"
        );
        ensure!(
            parser.codec_config().is_some(),
            "Missing probe codec configuration"
        );
        let sample = sample.context("No independent probe frame")?;
        Ok((
            times,
            crate::probe_format::sample(
                request.codec(),
                key.decoders.width,
                key.decoders.height,
                &sample,
            ),
        ))
    }
}
fn validate(key: &Key) -> Result<()> {
    ensure!(
        key.epoch > 0 && key.decoders.valid() && key.decoders.protocol == 2,
        "Invalid probe identity or decoder capabilities"
    );
    ensure!(
        key.encoder
            .as_ref()
            .is_none_or(|s| CANDIDATES.contains(&s.as_str())),
        "Unknown explicit Windows encoder"
    );
    blent_config::raw_frame::Layout::new(key.decoders.width, key.decoders.height, 2)?;
    requests(key)?;
    Ok(())
}
fn requests(key: &Key) -> Result<Vec<Request>> {
    // Probe software first so bounded automatic work always attempts fallback.
    let names = key.encoder.as_deref().map_or_else(
        || {
            std::iter::once("libx264")
                .chain(CANDIDATES.into_iter().filter(|s| *s != "libx264"))
                .collect()
        },
        |name| vec![name],
    );
    let mut result = Vec::new();
    for name in names {
        for workers in blent_config::encoder_workers::candidates(name, key.workers) {
            let format = if name.starts_with("hevc") {
                if key.ten_bit {
                    Format::Main10
                } else {
                    Format::Main
                }
            } else {
                Format::Baseline
            };
            let request = Request {
                encoder: name.into(),
                format,
                fps: key.decoders.fps,
                bitrate: key.bitrate,
                quality: key.quality,
                workers,
            };
            request.validate()?;
            result.push(request);
        }
    }
    blent_config::encoder_workers::validate(key.workers)?;
    Ok(result)
}
fn inventory(bytes: &[u8]) -> Result<Vec<String>> {
    ensure!(bytes.len() <= 1_048_576, "Oversized encoder inventory");
    let text = std::str::from_utf8(bytes)?;
    let mut names = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(flags) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        if flags.len() == 6
            && flags.starts_with('V')
            && CANDIDATES.contains(&name)
            && !names.iter().any(|s| s == name)
        {
            names.push(name.to_string());
        }
    }
    Ok(names)
}
async fn bounded_output(program: &std::ffi::OsStr, args: &[&str], limit: usize) -> Result<Vec<u8>> {
    crate::command_output::read(program, args, limit, DEADLINE).await
}

fn summarize(times: &[u64], stream: StreamProfile) -> Result<Measurement> {
    ensure!(times.len() == FRAMES, "Encoder probe lost frames");
    ensure!(
        times.windows(2).all(|p| p[0] <= p[1]),
        "Invalid packet timestamps"
    );
    let measured = &times[8..];
    let elapsed = (measured[measured.len() - 1] - measured[0]).max(1);
    let mut intervals: Vec<_> = measured.windows(2).map(|p| p[1] - p[0]).collect();
    intervals.sort_unstable();
    Ok(Measurement {
        first_us: times[0],
        p95_us: intervals[intervals.len() * 95 / 100],
        fps: (measured.len() - 1) as f64 * 1_000_000.0 / elapsed as f64,
        workers_requested: 0,
        workers_effective: None,
        stream,
    })
}

#[cfg(test)]
mod tests;
