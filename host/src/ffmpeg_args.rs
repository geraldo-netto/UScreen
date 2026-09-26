//! Shared stock-FFmpeg recipe. Paths and child lifecycle belong to adapters.
use crate::media::Codec;
use anyhow::Result;

pub const TEE_OUTPUT: &str = "[f=framecrc:flush_packets=1]pipe:1|[f=data:flush_packets=1]pipe:1";

pub struct Settings {
    pub encoder: String,
    pub vaapi_device: String,
    pub fps: u32,
    pub bitrate: u32,
    pub quality: u32,
    pub workers: u32,
    pub adaptive_idle: bool,
}

impl Settings {
    pub fn profile(&self) -> Result<blent_config::encoding::Profile> {
        blent_config::encoding::Profile::new(&self.encoder, self.fps, self.bitrate, self.quality)?
            .with_workers(self.workers)
    }
    pub fn arguments(
        &self,
        input: &std::ffi::OsStr,
        ten_bit: bool,
        w: u32,
        h: u32,
    ) -> Result<Vec<std::ffi::OsString>> {
        let encoder = blent_config::ffmpeg_encoder_name(&self.encoder);
        let codec = Codec::from_encoder(encoder);
        let fps = self.fps;
        let mut encoder_args: Vec<std::ffi::OsString> = vec!["-hide_banner".into()];

        if encoder.ends_with("_vaapi") {
            encoder_args
                .extend_from_slice(&["-vaapi_device".into(), self.vaapi_device.clone().into()]);
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
            input.to_owned(),
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
            if self.adaptive_idle {
                "expr:if(isnan(prev_forced_t),1,gte(t,prev_forced_t+0.9))".into()
            } else {
                "expr:if(isnan(prev_forced_t),1,gte(t,prev_forced_t+1))".into()
            },
        ]);

        let mut quality_args = Vec::new();
        quality_args.extend(
            self.profile()?
                .cli_options(ten_bit)
                .into_iter()
                .flat_map(|(key, value)| [key, value]),
        );
        encoder_args.extend(quality_args.into_iter().map(std::ffi::OsString::from));
        if codec.framed() {
            encoder_args.extend(
                ["-flush_packets", "1"]
                    .into_iter()
                    .map(std::ffi::OsString::from),
            );
        }
        if codec.framed() {
            encoder_args.extend_from_slice(&["-f".into(), "ivf".into(), "pipe:1".into()]);
        } else {
            // T448: tee visits the metadata slave first and flushes it before
            // the data slave writes this SAME pipe. Keep use_fifo disabled.
            // `data` preserves AVPacket bytes without an implicit bitstream filter.
            encoder_args.extend(
                ["-map", "0:v:0", "-f", "tee", TEE_OUTPUT]
                    .into_iter()
                    .map(std::ffi::OsString::from),
            );
        }
        Ok(encoder_args)
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
