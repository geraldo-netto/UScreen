//! Distribution FFmpeg CLI adapter and encoded stdout drain.
#[cfg(test)]
#[path = "timestamp_tests.rs"]
mod timestamp_tests;

#[cfg(test)]
#[path = "startup_packet_tests.rs"]
mod startup_packet_tests;

use super::{fifo_path_for, CaptureConfig};
use crate::annex_b::AnnexBPacketizer;
use crate::media::Codec;
use crate::media::CodecConfig;
use anyhow::{Context, Result};
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;
use tracing::{info, warn};

pub(super) struct CliEncoder<'a> {
    pub(super) config: &'a CaptureConfig,
}
impl CliEncoder<'_> {
    pub(super) fn log_encoder_dimensions(&self, w: u32, h: u32) {
        let n = self.config.stream_scale.max(1);
        let expected = (
            ((self.config.width / n) & !1).max(2),
            ((self.config.height / n) & !1).max(2),
        );
        if (w, h) != expected {
            warn!(
                "Encoding at {}x{}, expected {}x{} — the compositor did not honour \
                     the requested mode",
                w, h, expected.0, expected.1
            );
        } else if n > 1 {
            info!(
                "Encoding at {}x{} (desktop {}x{}, stream scale {})",
                w, h, self.config.width, self.config.height, n
            );
        }
    }

    pub(super) fn encoder_command(
        &self,
        w: u32,
        h: u32,
        async_depth_supported: bool,
    ) -> Result<Command> {
        let encoder = crate::config::ffmpeg_encoder_name(&self.config.encoder);
        let codec = Codec::from_encoder(encoder);
        // 10-bit only makes sense on HEVC here: NVENC's H.264 encoder is
        // 8-bit, so asking for it there would silently do nothing.
        let ten_bit = self.config.ten_bit && codec == Codec::Hevc;
        if self.config.ten_bit && !ten_bit {
            warn!(
                "10-bit was asked for but {} is 8-bit only — ignoring",
                encoder
            );
        }
        let mut encoder_args = self.encoder_arguments(encoder, codec, ten_bit, w, h)?;
        if !async_depth_supported {
            if let Some(index) = encoder_args.iter().position(|arg| arg == "-async_depth") {
                encoder_args.drain(index..index + 2);
            }
        }
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&encoder_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        Ok(cmd)
    }

    fn encoder_arguments(
        &self,
        encoder: &str,
        codec: Codec,
        ten_bit: bool,
        w: u32,
        h: u32,
    ) -> Result<Vec<std::ffi::OsString>> {
        let fps = self.config.fps;
        let mut encoder_args: Vec<std::ffi::OsString> = vec!["-hide_banner".into()];

        if encoder.ends_with("_vaapi") {
            encoder_args.extend_from_slice(&[
                "-vaapi_device".into(),
                self.config.vaapi_device.clone().into(),
            ]);
        }

        // T447: the raw format is fully specified. Probe at most one picture
        // and retain it; `nobuffer` discards that picture instead of reducing
        // the encoder's steady-state queue.
        encoder_args.extend(
            ["-probesize", "32", "-analyzeduration", "0"]
                .into_iter()
                .map(std::ffi::OsString::from),
        );
        encoder_args.extend_from_slice(&[
            "-flags".into(),
            "low_delay".into(),
            // The helper emits BT.709 limited-range NV12. Say so on the
            // INPUT side, before -i. Given as output options (where they used
            // to be), ffmpeg 7+ treats them as a request to *convert* an
            // untagged input to BT.709, auto-inserts a scale filter and runs
            // every frame through swscale via an RGB intermediate on the CPU:
            // measured ~4 cores at 60 fps, 2960x1848, against 0.4 without,
            // plus a full-frame conversion's worth of latency and a colour
            // shift from treating 709 data as 601. Tagged on the input, the
            // stream still carries bt709/tv in the SPS and nothing is
            // converted. Full range was tried and reverted — see the note in
            // evdi_helper.c.
            "-color_primaries".into(),
            "bt709".into(),
            "-color_trc".into(),
            "bt709".into(),
            "-colorspace".into(),
            "bt709".into(),
            "-color_range".into(),
            "tv".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            // The helper converts the BGRA framebuffer to NV12 before the
            // FIFO: 1.5 bytes/px instead of 4, so the raw-frame copies that
            // bottleneck the pipeline shrink 2.7x and NVENC takes it directly.
            "nv12".into(),
            "-s".into(),
            format!("{}x{}", w, h).into(),
            "-framerate".into(),
            fps.to_string().into(),
            "-use_wallclock_as_timestamps".into(),
            "1".into(),
            "-i".into(),
            fifo_path_for(self.config.instance)?.into_os_string(),
        ]);

        encoder_args.extend_from_slice(&[
            "-vf".into(),
            video_filter(encoder.ends_with("_vaapi"), ten_bit).into(),
            "-enc_time_base".into(),
            "1:1000000".into(),
            "-c:v".into(),
            encoder.into(),
            "-fps_mode".into(),
            "passthrough".into(),
            "-force_key_frames".into(),
            "expr:if(isnan(prev_forced_t),1,gte(t,prev_forced_t+1))".into(),
        ]);

        let mut quality_args = Vec::new();
        self.encoder_quality_args(&mut quality_args, &self.config.encoder, ten_bit)?;
        encoder_args.extend(quality_args.into_iter().map(std::ffi::OsString::from));
        if codec.framed() {
            encoder_args.extend(
                ["-flush_packets", "1"]
                    .into_iter()
                    .map(std::ffi::OsString::from),
            );
        }
        encoder_args.extend_from_slice(&["-f".into(), codec.muxer().into(), "pipe:1".into()]);
        Ok(encoder_args)
    }

    pub(super) fn encoder_quality_args(
        &self,
        args: &mut Vec<String>,
        encoder: &str,
        ten_bit: bool,
    ) -> Result<()> {
        let profile = uscreen_config::encoding::Profile::new(
            encoder,
            self.config.fps,
            self.config.bitrate,
            self.config.quality,
        )?;
        args.extend(
            profile
                .cli_options(ten_bit)
                .into_iter()
                .flat_map(|(key, value)| [key, value]),
        );
        Ok(())
    }
}

fn video_filter(vaapi: bool, ten_bit: bool) -> String {
    // T421: rawvideo quantizes wall-clock timestamps to 1/fps. Catch-up
    // frames can share PTS, and CLOCK_REALTIME may move backwards. Keep
    // their order with a minimum one-microsecond step, preserving sparse
    // wall-time gaps for periodic IDRs. These filters change metadata only.
    let timing = "settb=1/1000000,setpts='if(isnan(PREV_OUTPTS),PTS,max(PTS,PREV_OUTPTS+1))'";
    // Conversion/upload retains the single 8-bit NV12 FIFO contract.
    let conversion = match (vaapi, ten_bit) {
        (true, true) => ",format=p010le,hwupload",
        (true, false) => ",format=nv12,hwupload",
        (false, true) => ",format=p010le",
        (false, false) => "",
    };
    format!("{timing}{conversion}")
}
/// T400: distribution FFmpeg options vary; probe the selected stock encoder.
/// Uses the shared asynchronous deadline/cancellation implementation.
pub(super) async fn supports_async_depth(program: &std::ffi::OsStr, encoder: &str) -> bool {
    use uscreen_config::commands::AsyncCommandExt;
    if !encoder.ends_with("_vaapi") {
        return false;
    }
    let output = Command::new(program)
        .args(["-hide_banner", "-h", &format!("encoder={encoder}")])
        .output_bounded()
        .await;
    match output {
        Ok(output) if output.status.success() => {
            [&output.stdout, &output.stderr].iter().any(|bytes| {
                String::from_utf8_lossy(bytes)
                    .lines()
                    .any(|line| line.split_whitespace().next() == Some("-async_depth"))
            })
        }
        _ => false,
    }
}

pub(super) enum Packetizer {
    AnnexB(AnnexBPacketizer),
    Ivf(crate::ivf::IvfPacketizer),
}
impl Packetizer {
    pub(super) fn new(codec: Codec, latency: crate::latency::LatencyTracker) -> Self {
        if codec.framed() {
            Self::Ivf(crate::ivf::IvfPacketizer::new(codec, latency))
        } else {
            Self::AnnexB(AnnexBPacketizer::new(codec, latency))
        }
    }
    fn codec_config(&self) -> Option<crate::media_storage::MediaBytes> {
        match self {
            Self::AnnexB(p) => p.codec_config(),
            Self::Ivf(p) => p.codec_config(),
        }
    }
    pub(super) async fn read_from(
        &mut self,
        input: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> Result<(usize, Vec<crate::media::VideoPacket>)> {
        match self {
            Self::AnnexB(p) => p.read_from(input).await,
            Self::Ivf(p) => p.read_from(input).await,
        }
    }
}

pub(super) async fn read_loop(
    stdout: impl tokio::io::AsyncRead + Unpin,
    tx: crate::video_queue::VideoSender,
    codec_config: CodecConfig,
    latency: crate::latency::LatencyTracker,
    codec: Codec,
    evidence: std::sync::Arc<crate::latency::EncoderEvidence>,
) -> Result<()> {
    let mut total: u64 = 0;
    let mut frames: u64 = 0;
    let mut last_log = Instant::now();
    let mut stdout = tokio::io::BufReader::new(stdout);
    let mut packetizer = Packetizer::new(codec, latency.clone());
    let mut config_extracted = codec_config.current().is_some();

    loop {
        let (n, access_units) = packetizer
            .read_from(&mut stdout)
            .await
            .context("Read/assembly error from encoder")?;
        total += n as u64;

        if n > 0 {
            publish_initial_codec_config(&packetizer, &codec_config, &mut config_extracted, total);
        }

        for data in access_units {
            frames += 1;
            if tx.receiver_count() > 0 {
                latency.on_encoded_for(data.seq, &evidence);
                let _ = tx.send(data);
            }
        }
        if n == 0 {
            return Ok(());
        }
        latency.maybe_report();

        report_encoder_throughput(&mut frames, &mut total, &mut last_log);
    }
}

fn publish_initial_codec_config(
    packetizer: &Packetizer,
    codec_config: &CodecConfig,
    config_extracted: &mut bool,
    total: u64,
) {
    if !*config_extracted {
        if let Some(config) = packetizer.codec_config() {
            info!("Extracted codec configuration: {} bytes", config.len());
            codec_config.publish(Some(config));
            *config_extracted = true;
        } else if total > 1024 * 1024 {
            warn!("Could not find codec configuration in first 1MB of stream");
            *config_extracted = true;
        }
    }
}

fn encoder_throughput_rates(total: u64, elapsed: f64) -> (f64, f64) {
    let megabytes_per_second = if elapsed > 0.0 {
        (total as f64 / elapsed) / 1_000_000.0
    } else {
        0.0
    };
    (megabytes_per_second, megabytes_per_second * 8.0 * 1000.0)
}

fn report_encoder_throughput(frames: &mut u64, total: &mut u64, last_log: &mut Instant) {
    if last_log.elapsed().as_secs() >= 5 {
        let elapsed = last_log.elapsed().as_secs_f64();
        let (megabytes_per_second, kilobits_per_second) = encoder_throughput_rates(*total, elapsed);
        info!(
            "Encoder: {} access units in {:.1}s, {:.1} MB/s ({:.0} kbps)",
            frames, elapsed, megabytes_per_second, kilobits_per_second
        );
        *frames = 0;
        *total = 0;
        *last_log = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t307_encoder_throughput_uses_displayed_decimal_units() {
        for (bytes, seconds, megabytes, kilobits) in [
            (0, 5.0, 0.0, 0.0),
            (1_000_000, 1.0, 1.0, 8000.0),
            (37_500_000, 5.0, 7.5, 60000.0),
            (125_000, 0.5, 0.25, 2000.0),
            (1_000_000, 0.0, 0.0, 0.0),
        ] {
            assert_eq!(
                encoder_throughput_rates(bytes, seconds),
                (megabytes, kilobits),
                "T307: wrong MB/s and kbps for {bytes} bytes over {seconds} seconds"
            );
        }
    }
    #[test]
    fn t306_nvenc_preserves_kilobit_bitrate_limits() {
        for encoder in ["h264_nvenc", "hevc_nvenc"] {
            for bitrate in [1000, 1001, 1049, 1051, 19999, 20000, 59999, 60000] {
                let config = CaptureConfig {
                    bitrate,
                    ..Default::default()
                };
                let manager = CliEncoder { config: &config };
                let mut args = Vec::new();
                manager
                    .encoder_quality_args(&mut args, encoder, false)
                    .unwrap();
                let limit = &args.windows(2).find(|pair| pair[0] == "-maxrate").unwrap()[1];
                // Decode SI units independently; either exact k or M is valid.
                let (number, multiplier) = match limit.as_bytes().last() {
                    Some(b'k') => (&limit[..limit.len() - 1], 1_000.0),
                    Some(b'M') => (&limit[..limit.len() - 1], 1_000_000.0),
                    _ => (limit.as_str(), 1.0),
                };
                assert_eq!(
                    number.parse::<f64>().unwrap() * multiplier,
                    f64::from(bitrate) * 1000.0,
                    "T306: {encoder} rounded {bitrate} kbps to {limit}"
                );
            }
        }
    }
    #[tokio::test]
    async fn t228_sequences_survive_encoder_restarts_before_ack() {
        let latency = crate::latency::LatencyTracker::new();
        let (tx, mut rx) = crate::video_queue::channel(8, Default::default());
        let input = [
            vec![0, 0, 0, 1, 5, 0x80, 0x11],
            vec![0, 0, 0, 1, 1, 0x80, 0x22],
        ]
        .concat();
        let mut sequences = Vec::new();
        for _ in 0..2 {
            read_loop(
                input.as_slice(),
                tx.clone(),
                CodecConfig::default(),
                latency.clone(),
                Codec::H264,
                latency.encoder_started("fixture", (0, 0, 0, 0, 0)),
            )
            .await
            .unwrap();
            sequences.push(rx.recv().await.unwrap().seq);
            sequences.push(rx.recv().await.unwrap().seq);
        }
        assert_eq!(
            sequences,
            vec![0, 1, 2, 3],
            "T228: restarted encoders must not reuse pending ACK identifiers"
        );
        latency.on_rendered(sequences[2], 0);
        latency.on_rendered(sequences[0], 0); // delayed ACK from retired encoder
        latency.on_rendered(sequences[3], 0);
    }
}
#[cfg(test)]
mod encoder_policy_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn cli_pairs(args: &[String]) -> BTreeMap<&str, &str> {
        let pairs = args
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| (p[0].as_str(), p[1].as_str()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            pairs.len() * 2,
            args.len(),
            "T373: duplicate or incomplete option"
        );
        pairs
    }

    #[test]
    fn t400_baseline_profile_reaches_stock_vaapi_command() {
        let config = CaptureConfig {
            encoder: "h264_vaapi_baseline".into(),
            ten_bit: true,
            vaapi_device: "/dev/dri/renderD129".into(),
            instance: u32::MAX,
            ..Default::default()
        };
        for supported in [false, true] {
            let command = CliEncoder { config: &config }
                .encoder_command(640, 480, supported)
                .unwrap();
            let args = command
                .as_std()
                .get_args()
                .map(|arg| arg.to_str().unwrap())
                .collect::<Vec<_>>();
            let value = |flag| {
                args.windows(2)
                    .rfind(|pair| pair[0] == flag)
                    .map(|pair| pair[1])
            };
            assert_eq!(value("-c:v"), Some("h264_vaapi"));
            assert_eq!(value("-profile:v"), Some("constrained_baseline"));
            assert_eq!(value("-coder"), Some("cavlc"));
            assert_eq!(value("-vaapi_device"), Some("/dev/dri/renderD129"));
            assert_eq!(value("-vf"), Some("settb=1/1000000,setpts='if(isnan(PREV_OUTPTS),PTS,max(PTS,PREV_OUTPTS+1))',format=nv12,hwupload"));
            assert_eq!(value("-async_depth"), supported.then_some("1"));
            assert_eq!(value("-f"), Some("h264"));
        }
    }

    #[tokio::test]
    async fn t400_optional_control_uses_successful_encoder_help() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("ffmpeg");
        for (help, status, stream, expected) in [
            ("  -async_depth <int> E..V. processing depth", 0, "1", true),
            ("  -async_depth <int> E..V. processing depth", 0, "2", true),
            ("  -idr_interval <int> E..V. IDR interval", 0, "1", false),
            ("Unknown option -async_depth", 0, "1", false),
            ("  -async_depth <int> E..V. processing depth", 1, "1", false),
        ] {
            std::fs::write(&program, format!("#!/bin/sh\n[ \"$1 $2 $3\" = '-hide_banner -h encoder=h264_vaapi' ] || exit 2\nprintf '%s\\n' '{help}' >&{stream}\nexit {status}\n")).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(
                supports_async_depth(program.as_os_str(), "h264_vaapi").await,
                expected,
                "T400: {help}/{status}/{stream}"
            );
        }
        std::fs::remove_file(&program).unwrap();
        assert!(!supports_async_depth(program.as_os_str(), "libx264").await);
        assert!(!supports_async_depth(program.as_os_str(), "hevc_vaapi").await);
    }

    #[test]
    fn t400_unsupported_vaapi_omits_optional_depth() {
        for encoder in ["h264_vaapi", "hevc_vaapi"] {
            let config = CaptureConfig {
                encoder: encoder.into(),
                instance: u32::MAX,
                ..Default::default()
            };
            let manager = CliEncoder { config: &config };
            let command = manager.encoder_command(640, 480, false).unwrap();
            assert!(
                command.as_std().get_args().all(|arg| arg != "-async_depth"),
                "T400: older stock FFmpeg must not receive unsupported optional controls"
            );
        }
    }

    #[test]
    fn t373_cli_quality_profiles_keep_adapter_contracts() {
        for (fps, bitrate, quality, nvbuf, swbuf) in
            [(10, 1000, 12, 200, 200), (90, 60000, 32, 666, 1333)]
        {
            for encoder in [
                "h264_nvenc",
                "hevc_nvenc",
                "h264_vaapi",
                "hevc_vaapi",
                "libx264",
            ] {
                let config = CaptureConfig {
                    fps,
                    bitrate,
                    quality,
                    instance: u32::MAX,
                    ..Default::default()
                };
                let manager = CliEncoder { config: &config };
                for ten_bit in [false, true] {
                    let mut args = Vec::new();
                    manager
                        .encoder_quality_args(&mut args, encoder, ten_bit)
                        .unwrap();
                    let expected =
                        cli_expected(encoder, fps, bitrate, quality, nvbuf, swbuf, ten_bit);
                    let expected = expected
                        .split_whitespace()
                        .map(str::to_string)
                        .collect::<Vec<_>>();
                    assert_eq!(
                        cli_pairs(&args),
                        cli_pairs(&expected),
                        "T373: {encoder}, {fps}, {ten_bit}"
                    );
                }
            }
        }
    }

    // Fixed boundary expectations characterize the existing adapter before sharing policy.
    fn cli_expected(
        name: &str,
        fps: u32,
        bitrate: u32,
        quality: u32,
        nvbuf: u32,
        swbuf: u32,
        ten_bit: bool,
    ) -> String {
        let limits = format!("-maxrate {bitrate}k -g {fps}");
        if name.ends_with("_nvenc") {
            let depth = if ten_bit { "-profile:v main10" } else { "" };
            format!("-preset p1 -tune ull -zerolatency 1 -delay 0 -bf 0 -rc-lookahead 0 -multipass 0 -rc vbr -cq {quality} -b:v 0 -bufsize {nvbuf}k -forced-idr 1 {limits} {depth}")
        } else if name.ends_with("_vaapi") {
            // T400: live VAAPI output must not inherit FFmpeg's two-frame
            // asynchronous queue. Preserve this command-boundary regression.
            format!("-rc_mode CQP -qp {quality} -bf 0 -idr_interval 0 -async_depth 1 {limits}")
        } else {
            format!("-preset ultrafast -tune zerolatency -crf {quality} -bufsize {swbuf}k -x264-params scenecut=0 {limits}")
        }
    }

    #[test]
    fn t373_cli_color_depth_and_periodic_idr_remain_explicit() {
        for encoder in [
            "h264_nvenc",
            "hevc_nvenc",
            "h264_vaapi",
            "hevc_vaapi",
            "libx264",
        ] {
            for ten_bit in [false, true] {
                let config = CaptureConfig {
                    encoder: encoder.into(),
                    ten_bit,
                    instance: u32::MAX,
                    ..Default::default()
                };
                let manager = CliEncoder { config: &config };
                let command = manager.encoder_command(640, 480, true).unwrap();
                let args = command
                    .as_std()
                    .get_args()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                assert_input_colors(&args);
                let value = |flag| {
                    args.windows(2)
                        .find(|p| p[0] == flag)
                        .map(|p| p[1].as_str())
                };
                assert_eq!(
                    value("-force_key_frames"),
                    Some("expr:if(isnan(prev_forced_t),1,gte(t,prev_forced_t+1))")
                );
                let depth = ten_bit && encoder.starts_with("hevc");
                let filter = expected_filter(encoder.ends_with("_vaapi"), depth);
                let timing = "settb=1/1000000,setpts='if(isnan(PREV_OUTPTS),PTS,max(PTS,PREV_OUTPTS+1))'";
                let full = value("-vf").expect("T421: all codecs need monotonic timestamps");
                let suffix = full.strip_prefix(timing).expect("T421: unexpected timing filter");
                assert_eq!(suffix.strip_prefix(','), filter, "T373: {encoder}, {ten_bit}");
            }
        }
    }

    fn expected_filter(vaapi: bool, ten_bit: bool) -> Option<&'static str> {
        match (vaapi, ten_bit) {
            (true, true) => Some("format=p010le,hwupload"),
            (true, false) => Some("format=nv12,hwupload"),
            (false, true) => Some("format=p010le"),
            (false, false) => None,
        }
    }

    fn assert_input_colors(args: &[String]) {
        let input = args.iter().position(|a| a == "-i").unwrap();
        for (flag, value) in [
            ("-color_primaries", "bt709"),
            ("-color_trc", "bt709"),
            ("-colorspace", "bt709"),
            ("-color_range", "tv"),
            ("-pix_fmt", "nv12"),
        ] {
            assert!(
                args[..input].windows(2).any(|p| p == [flag, value]),
                "T373: input {flag}"
            );
        }
    }
}

#[cfg(test)]
#[path = "../packetizer_limits_tests.rs"]
mod packetizer_limits_tests;

#[cfg(test)]
mod framed_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn t432_stock_cli_emits_sparse_vp9_without_next_frame_or_eof() {
        sparse_frames("libvpx-vp9").await;
    }

    #[tokio::test]
    async fn t433_stock_cli_emits_sparse_av1_without_next_frame_or_eof() {
        sparse_frames("libaom-av1").await;
    }

    async fn sparse_frames(encoder: &str) {
        let config = CaptureConfig {
            encoder: encoder.into(),
            fps: 60,
            instance: u32::MAX,
            ..Default::default()
        };
        let built = CliEncoder { config: &config }
            .encoder_command(64, 64, false)
            .unwrap();
        let mut args = built
            .as_std()
            .get_args()
            .map(std::ffi::OsStr::to_owned)
            .collect::<Vec<_>>();
        let input = args.iter().position(|arg| arg == "-i").unwrap();
        args[input + 1] = "pipe:0".into();
        let mut child = Command::new("ffmpeg")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
        let mut parser =
            crate::ivf::IvfPacketizer::new(Codec::from_encoder(encoder), Default::default());
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            // T447: retain the probe picture too. Every input, starting with
            // the first, must produce output while stdin stays open.
            stdin.write_all(&vec![80; 64 * 64 * 3 / 2]).await.unwrap();
            let first = parser.read_from(&mut stdout).await.unwrap().1;
            assert!(first[0].is_idr);
            for value in [100, 120, 140] {
                stdin
                    .write_all(&vec![value; 64 * 64 * 3 / 2])
                    .await
                    .unwrap();
                let frame = parser.read_from(&mut stdout).await.unwrap().1;
                assert_eq!(
                    frame.len(),
                    1,
                    "T432/T433: sparse frame must not wait for its successor"
                );
            }
        })
        .await;
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        assert!(
            outcome.is_ok(),
            "T432/T433: stock CLI retained a frame or failed to encode"
        );
    }
}
