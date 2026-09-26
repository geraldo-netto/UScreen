//! Isolated production-profile throughput probes. Never create a display/FIFO.
use super::{
    cli_encoder::{CliEncoder, Packetizer},
    CaptureConfig,
};
use anyhow::{ensure, Context, Result};
use std::{
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(Clone, Debug)]
pub(crate) struct Measurement {
    pub workers_requested: u32,
    pub workers_effective: Option<u32>,
    pub encoder: String,
    pub first_us: u64,
    pub p95_us: u64,
    pub fps: f64,
    pub stream: Option<blent_config::negotiation::StreamProfile>,
    /// First deterministic probe frame only; not a perceptual/full-corpus score.
    pub quality_db: Option<f64>,
}

pub(crate) async fn measure(config: &CaptureConfig) -> Result<Measurement> {
    // Includes startup, input, drain, wait and failure cleanup. kill_on_drop
    // retires the isolated child on deadline or selection cancellation.
    tokio::time::timeout(Duration::from_secs(8), run(config))
        .await
        .context("Encoder probe exceeded eight seconds")?
}

async fn run(config: &CaptureConfig) -> Result<Measurement> {
    let (w, h) = (config.width, config.height);
    ensure!(
        (2..=4096).contains(&w) && (2..=4096).contains(&h),
        "Invalid probe dimensions"
    );
    let codec = crate::media::Codec::from_encoder(&config.encoder);
    let depth = super::cli_encoder::supports_async_depth(
        std::ffi::OsStr::new("ffmpeg"),
        crate::config::ffmpeg_encoder_name(&config.encoder),
    )
    .await;
    let mut child = command(config, depth)?
        .spawn()
        .context("Start isolated encoder probe")?;
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let started = Instant::now();
    let ((), (times, sample)) = tokio::try_join!(feed(stdin, w, h), drain(stdout, codec, started))?;
    ensure!(child.wait().await?.success(), "Encoder probe failed");
    let mut measured = summarize(&config.encoder, &times)?;
    measured.workers_requested = if config.encoder == "libx264" { config.worker_count() } else { 0 };
    if let Some(sample) = sample {
        if config.encoder == "libx264" {
            measured.workers_effective = blent_config::encoder_workers::effective_x264(&sample.data);
        }
        measured.stream = super::probe_format::inspect(codec, w, h, &sample)
            .await
            .ok();
        measured.quality_db = super::probe_quality::inspect(codec, w, h, &sample)
            .await
            .ok();
    }
    Ok(measured)
}

fn command(config: &CaptureConfig, depth: bool) -> Result<Command> {
    let built = CliEncoder { config }.encoder_command(config.width, config.height, depth)?;
    let mut args = built
        .as_std()
        .get_args()
        .map(std::ffi::OsStr::to_owned)
        .collect::<Vec<_>>();
    let input = args.iter().position(|arg| arg == "-i").unwrap();
    args[input + 1] = "pipe:0".into();
    // T447: production already bounds raw probing and retains the probe frame.
    // Startup is not measured: steady packet cadence is the ranking evidence.
    let mut command = Command::new("ffmpeg");
    command
        .args(["-loglevel", "error"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    Ok(command)
}

async fn feed(mut output: tokio::process::ChildStdin, width: u32, height: u32) -> Result<()> {
    let mut frame = reference(width, height);
    for index in 0..65 {
        let row = index % height as usize;
        frame[row * width as usize..(row + 1) * width as usize].fill((16 + index * 3) as u8);
        output.write_all(&frame).await?;
    }
    output.shutdown().await?;
    Ok(())
}

pub(super) fn reference(width: u32, height: u32) -> Vec<u8> {
    let pixels = width as usize * height as usize;
    let mut frame = vec![128u8; pixels * 3 / 2];
    // Deterministic mixed-detail luma; reuse a single frame allocation. Every
    // frame moves a stripe, so temporal encoders cannot benchmark only repeats.
    for (index, value) in frame[..pixels].iter_mut().enumerate() {
        let (x, y) = (index % width as usize, index / width as usize);
        *value = (16 + (((x / 8) ^ (y / 8)) * 37 % 220)) as u8;
    }
    frame[..width as usize].fill(16);
    frame
}

async fn drain(
    stdout: tokio::process::ChildStdout,
    codec: crate::media::Codec,
    started: Instant,
) -> Result<(Vec<u64>, Option<crate::media::VideoPacket>)> {
    let mut input = tokio::io::BufReader::new(stdout);
    let mut parser = Packetizer::new(codec, Default::default());
    let mut times = Vec::with_capacity(65);
    let mut sample = None;
    loop {
        let (read, frames) = parser.read_from(&mut input).await?;
        ensure!(
            times.len() + frames.len() <= 65,
            "Unexpected probe frame count"
        );
        times.extend(frames.iter().map(|_| started.elapsed().as_micros() as u64));
        if sample.is_none() {
            sample = frames.into_iter().find(|frame| frame.is_idr);
        }
        if read == 0 {
            return Ok((times, sample));
        }
    }
}

fn summarize(encoder: &str, times: &[u64]) -> Result<Measurement> {
    ensure!(times.len() >= 60, "Encoder probe lost frames");
    let measured = &times[8..];
    let elapsed = (measured.last().unwrap() - measured[0]).max(1);
    let mut intervals = measured
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect::<Vec<_>>();
    intervals.sort_unstable();
    Ok(Measurement {
        workers_requested: 0,
        workers_effective: None,
        encoder: encoder.into(),
        stream: None,
        quality_db: None,
        first_us: times[0],
        p95_us: intervals[(intervals.len() * 95 / 100).min(intervals.len() - 1)],
        fps: (measured.len() - 1) as f64 * 1_000_000.0 / elapsed as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn t434_production_profiles_measure_stock_encoders_without_capture() {
        for encoder in ["libx264", "libvpx-vp9", "libaom-av1"] {
            let config = CaptureConfig {
                encoder: encoder.into(),
                width: 64,
                height: 64,
                instance: u32::MAX,
                ..Default::default()
            };
            let measured = measure(&config)
                .await
                .expect("T434: supported stock probe failed");
            assert_eq!(measured.encoder, encoder);
            assert!(measured.fps.is_finite() && measured.fps > 0.0);
            assert!(measured.first_us > 0);
            assert!(
                measured
                    .quality_db
                    .is_some_and(|q| q.is_finite() && q > 0.0),
                "T479: stock probe first-frame quality missing for {encoder}"
            );
            if encoder == "libx264" {
                let actual = measured
                    .stream
                    .expect("T478: stock H.264 profile inspection missing");
                assert_eq!(actual.format.profile, "constrained-baseline");
                assert_eq!(actual.format.depth, 8);
            }
        }
    }

    #[tokio::test]
    async fn t434_absent_encoder_and_missing_gpu_are_rejected() {
        let mut config = CaptureConfig {
            encoder: "absent-t434-encoder".into(),
            width: 64,
            height: 64,
            instance: u32::MAX,
            ..Default::default()
        };
        assert!(measure(&config).await.is_err());
        config.encoder = "vp9_vaapi".into();
        config.vaapi_device = "/nonexistent/blent-t434-render-node".into();
        assert!(measure(&config).await.is_err());
    }

    #[test]
    fn t434_probe_statistics_exclude_warmup_and_reject_lost_frames() {
        let mut times = (0..64).map(|n| 100_000 + n * 500).collect::<Vec<_>>();
        times[0] = 10;
        let measured = summarize("fixture", &times).unwrap();
        assert_eq!(measured.p95_us, 500);
        assert_eq!(measured.fps, 2000.0);
        assert!(summarize("fixture", &times[..12]).is_err());
    }

    #[tokio::test]
    async fn t434_cancelling_probe_reaps_its_isolated_ffmpeg() {
        let config = CaptureConfig {
            encoder: "libx264".into(),
            width: 64,
            height: 64,
            instance: u32::MAX,
            ..Default::default()
        };
        let child = command(&config, false).unwrap().spawn().unwrap();
        let pid = child.id().unwrap();
        let task = tokio::spawn(async move {
            let _owned = child;
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(2), async {
            while std::path::Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("T434: cancelled probe left an encoder process");
    }
}
