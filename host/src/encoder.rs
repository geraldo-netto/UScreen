//! In-process H.264 encoding through libavcodec.
//!
//! The default CLI path sends packed NV12 through a FIFO to stock FFmpeg.
//! This optional encoder accepts FIFO input or read-only shared slot leases.
//! Automatic libx264 input uses the shared adapter validated by T418.
//! FIFO aligned frames fill writable AVFrame planes; padded widths use staging.
//! Shared AVBufferRefs retain each slot until the codec's final reference ends.
//! Neither adapter changes FFmpeg itself or establishes hardware availability.
//!
//! In-process submission can request an IDR on the next captured frame. The
//! CLI instead schedules periodic wall-clock IDRs. See the raw-input benchmark
//! for measured copy and transport costs; FFmpeg itself is not modified.
//!
//! Built only with the `inproc-encoder` feature; see host/Cargo.toml for why.

use crate::encoder_io::{extract_parameter_sets, FifoReader, StopSignal};
use crate::media::CodecConfig;
use crate::media_storage::MediaBytes as Bytes;
use anyhow::{Context, Result};

#[path = "encoder_frame.rs"]
mod input_frame;
#[path = "encoder_shared.rs"]
mod shared;
#[path = "encoder_storage.rs"]
mod storage;

pub struct Encoder {
    inner: ffmpeg_next::encoder::Video,
    frame: ffmpeg_next::frame::Video,
    pts: i64,
    // Fields drop in declaration order: the codec must close/join callbacks
    // before their shared callback context is retired.
    packet_storage: std::sync::Arc<storage::PacketStorage>,
}

impl Encoder {
    /// `name` is an libavcodec encoder name such as `h264_nvenc`.
    pub fn new(
        name: &str,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
        quality: u32,
    ) -> Result<Self> {
        crate::config::validate_encoder_for_build(name)?;
        let profile = blent_config::encoding::Profile::new(name, fps, bitrate_kbps, quality)?;
        ffmpeg_next::init().context("initialise libavcodec")?;

        let codec = ffmpeg_next::encoder::find_by_name(name)
            .with_context(|| format!("encoder {} not available in this libavcodec", name))?;

        // Declare storage first so error paths also close the context first.
        let packet_storage = std::sync::Arc::new(storage::PacketStorage::default());
        let mut ctx = ffmpeg_next::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()
            .context("open video encoder")?;
        packet_storage.install(&mut ctx);

        ctx.set_width(width);
        ctx.set_height(height);
        ctx.set_format(ffmpeg_next::format::Pixel::NV12);
        ctx.set_time_base(ffmpeg_next::Rational(1, fps.max(1) as i32));
        ctx.set_frame_rate(Some(ffmpeg_next::Rational(fps.max(1) as i32, 1)));
        // One nominal second in frames; longer at idle. Client joins can
        // request an IDR on the next captured frame.
        ctx.set_gop(profile.gop);
        ctx.set_max_b_frames(0);
        ctx.set_bit_rate(0);
        ctx.set_max_bit_rate(profile.max_rate_bps as usize);
        ctx.set_colorspace(ffmpeg_next::color::Space::BT709);
        ctx.set_color_range(ffmpeg_next::color::Range::MPEG);
        ctx.set_color_primaries(ffmpeg_next::color::Primaries::BT709);
        ctx.set_color_transfer_characteristic(ffmpeg_next::color::TransferCharacteristic::BT709);

        let mut opts = ffmpeg_next::Dictionary::new();
        Self::low_latency_options(&profile, &mut opts)?;

        let inner = ctx
            .open_with(opts)
            .with_context(|| format!("configure {}", name))?;

        let mut frame =
            ffmpeg_next::frame::Video::new(ffmpeg_next::format::Pixel::NV12, width, height);
        frame.set_color_space(ffmpeg_next::color::Space::BT709);
        frame.set_color_range(ffmpeg_next::color::Range::MPEG);
        frame.set_color_primaries(ffmpeg_next::color::Primaries::BT709);
        frame.set_color_transfer_characteristic(ffmpeg_next::color::TransferCharacteristic::BT709);

        Ok(Self {
            inner,
            frame,
            pts: 0,
            packet_storage,
        })
    }

    /// Translate shared policy to libavcodec dictionary values. Context fields stay typed.
    fn low_latency_options(
        profile: &blent_config::encoding::Profile,
        opts: &mut ffmpeg_next::Dictionary,
    ) -> Result<()> {
        for (key, value) in profile.inproc_options()? {
            opts.set(key, &value);
        }
        Ok(())
    }

    /// Feed one packed NV12 frame and collect whatever access units come out.
    ///
    /// `force_idr` makes the next frame a keyframe, which is what lets a client
    /// that just connected start decoding immediately instead of waiting for
    /// the next scheduled one.
    #[cfg(test)]
    pub fn encode(&mut self, nv12: &[u8], force_idr: bool) -> Result<Vec<(Bytes, bool)>> {
        let (w, h) = (self.frame.width() as usize, self.frame.height() as usize);
        if nv12.len() < w * h * 3 / 2 {
            anyhow::bail!("short NV12 frame: {} bytes for {}x{}", nv12.len(), w, h);
        }

        input_frame::writable(&mut self.frame)?;
        input_frame::copy_nv12(&mut self.frame, nv12);
        self.encode_prepared(force_idr)
    }

    /// Submit a complete input frame after its exclusive writable borrow ends.
    fn encode_prepared(&mut self, force_idr: bool) -> Result<Vec<(Bytes, bool)>> {
        self.frame.set_pts(Some(self.pts));
        self.pts += 1;
        if force_idr {
            self.frame.set_kind(ffmpeg_next::picture::Type::I);
        } else {
            self.frame.set_kind(ffmpeg_next::picture::Type::None);
        }

        self.inner.send_frame(&self.frame).context("send frame")?;
        self.drain()
    }

    fn drain(&mut self) -> Result<Vec<(Bytes, bool)>> {
        let mut out = Vec::new();
        let mut packet = ffmpeg_next::Packet::empty();
        loop {
            match self.inner.receive_packet(&mut packet) {
                Ok(()) => {}
                Err(ffmpeg_next::Error::Eof) => break,
                Err(ffmpeg_next::Error::Other {
                    errno: libc::EAGAIN,
                }) => break,
                Err(error) => return Err(error).context("receive encoded packet"),
            }
            let is_idr = packet.is_key();
            let received = std::mem::replace(&mut packet, ffmpeg_next::Packet::empty());
            if let Some(data) = self.packet_storage.payload(received) {
                out.push((data, is_idr));
            }
        }
        Ok(out)
    }
}

/// Read complete NV12 input frames or shared leases and publish access units.
/// FIFO reads cross the kernel pipe boundary; shared leases retain producer
/// slots through the final AVBufferRef. Padding, cancellation and generation
/// ownership belong to the input adapter, independently of codec submission.
// Blocking thread boundary takes owned session settings and channel handles.
#[allow(clippy::too_many_arguments)]
pub fn run(
    fifo_path: &std::path::Path,
    encoder_name: &str,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_kbps: u32,
    quality: u32,
    tx: crate::video_queue::VideoSender,
    codec_config: CodecConfig,
    idr_wanted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stop: std::sync::Arc<StopSignal>,
    latency: crate::latency::LatencyTracker,
    raw_socket: Option<crate::raw_socket::Socket>,
    raw_slots: u32,
) -> Result<()> {
    use std::sync::atomic::Ordering;

    let mut enc = Encoder::new(encoder_name, width, height, fps, bitrate_kbps, quality)?;

    let mut input = shared::Input::open(fifo_path, raw_socket, (width, height), raw_slots)?;
    tracing::info!(
        "In-process encoder running: {} at {}x{}",
        encoder_name,
        width,
        height
    );

    let generation = crate::media::EncoderGeneration::new();
    let evidence =
        latency.encoder_started(encoder_name, (width, height, fps, bitrate_kbps, quality));

    while !stop.requested() {
        match input.read(&mut enc.frame, &stop) {
            Ok(true) => {}
            Ok(false) => break, // asked to stop mid-frame
            Err(e) => return Err(e).context("read raw capture frame"),
        }

        let force = idr_wanted.swap(false, Ordering::Relaxed);
        for (data, is_idr) in enc.encode_prepared(force)? {
            if is_idr {
                refresh_codec_config(&data, encoder_name, &codec_config);
            }
            let seq = latency.next_sequence();
            if tx.receiver_count() > 0 {
                latency.on_encoded_for(seq, &evidence);
                let _ = tx.send(crate::media::VideoPacket {
                    data,
                    is_idr,
                    seq,
                    codec_config: codec_config.current(),
                    generation: generation.active.clone(),
                });
            }
        }
        latency.maybe_report();
    }
    Ok(())
}

fn refresh_codec_config(data: &[u8], encoder_name: &str, codec_config: &CodecConfig) {
    if let Some(config) =
        extract_parameter_sets(data, crate::media::Codec::from_encoder(encoder_name))
    {
        codec_config.publish(Some(config));
    }
}

#[cfg(test)]
#[path = "encoder_packet_tests.rs"]
mod packet_tests;
#[cfg(test)]
#[path = "raw_tests.rs"]
mod raw_tests;

#[cfg(test)]
#[path = "encoder_storage_replay.rs"]
mod storage_replay;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};

    #[test]
    fn t373_inproc_dictionary_preserves_boundary_policy_and_adapter_differences() {
        use std::collections::BTreeMap;
        for (fps, bitrate, quality, nvbuf, swbuf) in [
            (10, 1000, 12, 200000, 200000),
            (90, 60000, 32, 666000, 1333000),
        ] {
            for name in ["h264_nvenc", "hevc_nvenc", "libx264"] {
                let mut options = ffmpeg_next::Dictionary::new();
                let profile =
                    blent_config::encoding::Profile::new(name, fps, bitrate, quality).unwrap();
                Encoder::low_latency_options(&profile, &mut options).unwrap();
                let actual = options.iter().collect::<BTreeMap<_, _>>();
                let expected = if name.ends_with("_nvenc") {
                    format!("preset p1 tune ull zerolatency 1 delay 0 rc vbr multipass 0 rc-lookahead 0 forced-idr 1 cq {quality} bufsize {nvbuf}")
                } else {
                    format!("preset ultrafast tune zerolatency crf {quality} bufsize {swbuf}")
                };
                let pairs = expected.split_whitespace().collect::<Vec<_>>();
                let expected = pairs
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (p[0], p[1]))
                    .collect::<BTreeMap<_, _>>();
                assert_eq!(actual, expected, "T373: {name}, {fps}");
            }
        }
    }

    #[test]
    fn t284_vaapi_reports_build_limit_before_codec_initialization() {
        for name in [
            "h264_vaapi",
            "h264_vaapi_baseline",
            "hevc_vaapi",
            "vaapih264enc",
        ] {
            let error = Encoder::new(name, 64, 64, 60, 500, 20)
                .err()
                .expect("T284: VAAPI must be rejected");
            assert!(
                error
                    .to_string()
                    .contains("VAAPI is unavailable in this in-process build"),
                "T284: {error:#}"
            );
            assert!(error
                .to_string()
                .contains("without --features inproc-encoder"));
        }
    }

    #[test]
    fn t310_encoded_bitstream_preserves_bt709_color_description() {
        use ffmpeg_next::color::{Primaries, Range, Space, TransferCharacteristic};
        let mut encoder = Encoder::new("libx264", 64, 64, 60, 500, 20).unwrap();
        let packets = encoder.encode(&vec![128; 64 * 64 * 3 / 2], true).unwrap();
        assert!(!packets.is_empty());
        let codec = ffmpeg_next::decoder::find(ffmpeg_next::codec::Id::H264).unwrap();
        let mut decoder = ffmpeg_next::codec::context::Context::new()
            .decoder()
            .open_as(codec)
            .unwrap()
            .video()
            .unwrap();
        for (data, _) in packets {
            decoder
                .send_packet(&ffmpeg_next::Packet::copy(&data))
                .unwrap();
        }
        decoder.send_eof().unwrap();
        let mut decoded = ffmpeg_next::frame::Video::empty();
        decoder.receive_frame(&mut decoded).unwrap();
        assert_eq!(decoded.color_space(), Space::BT709, "T310 matrix");
        assert_eq!(decoded.color_range(), Range::MPEG, "T310 limited range");
        assert_eq!(
            decoded.color_primaries(),
            Primaries::BT709,
            "T310 primaries"
        );
        assert_eq!(
            decoded.color_transfer_characteristic(),
            TransferCharacteristic::BT709,
            "T310 transfer characteristic"
        );
    }

    #[test]
    fn t266_reused_input_preserves_retained_frame_planes() {
        let mut encoder = Encoder::new("libx264", 64, 64, 60, 500, 20).unwrap();
        encoder.encode(&vec![64; 64 * 64 * 3 / 2], true).unwrap();
        let mut retained = ffmpeg_next::frame::Video::empty();
        // Model libavcodec retaining input using its real reference-counted buffers.
        unsafe {
            assert_eq!(
                ffmpeg_next::ffi::av_frame_ref(retained.as_mut_ptr(), encoder.frame.as_ptr()),
                0
            );
            assert_eq!(
                ffmpeg_next::ffi::av_frame_is_writable(encoder.frame.as_mut_ptr()),
                0
            );
        }
        encoder.encode(&vec![192; 64 * 64 * 3 / 2], false).unwrap();
        for plane in 0..2 {
            for row in 0..(64 >> plane) {
                let old = row * retained.stride(plane);
                let new = row * encoder.frame.stride(plane);
                assert_eq!(
                    &retained.data(plane)[old..old + 64],
                    &[64; 64],
                    "T266: a retained frame changed after the next submission"
                );
                assert_eq!(&encoder.frame.data(plane)[new..new + 64], &[192; 64]);
            }
        }
    }

    #[test]
    fn t265_drain_reports_codec_errors() {
        ffmpeg_next::init().unwrap();
        // A real libavcodec EINVAL exercises the failure path without hardware.
        let context = ffmpeg_next::codec::context::Context::new()
            .encoder()
            .video()
            .unwrap();
        let mut encoder = Encoder {
            inner: ffmpeg_next::codec::encoder::video::Encoder(context),
            frame: ffmpeg_next::frame::Video::empty(),
            pts: 0,
            packet_storage: Default::default(),
        };
        let error = encoder
            .drain()
            .expect_err("T265: receive failures must reach the supervisor");
        assert_eq!(
            error.downcast_ref::<ffmpeg_next::Error>(),
            Some(&ffmpeg_next::Error::Other {
                errno: libc::EINVAL
            })
        );
    }

    #[test]
    fn t265_drain_preserves_packets_and_normal_exhaustion() {
        let mut encoder = Encoder::new("libx264", 64, 64, 60, 500, 20).unwrap();
        assert!(
            encoder.drain().unwrap().is_empty(),
            "T265: EAGAIN is normal"
        );
        let packets = encoder.encode(&vec![128; 64 * 64 * 3 / 2], true).unwrap();
        assert!(!packets.is_empty());
        assert!(packets.iter().any(|(data, key)| !data.is_empty() && *key));
        encoder.inner.send_eof().unwrap();
        assert!(encoder.drain().unwrap().is_empty(), "T265: EOF is normal");
    }

    fn encode_one_from_fifo(
        latency: crate::latency::LatencyTracker,
        name: &std::ffi::OsStr,
    ) -> u32 {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        use std::os::unix::ffi::OsStrExt;
        let fifo = dir.path().join(name);
        let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let (tx, mut rx) = crate::video_queue::channel(8, Default::default());
        let stop = StopSignal::new().unwrap();
        let stopped = stop.clone();
        let writer_path = fifo.clone();
        let task = std::thread::spawn(move || {
            run(
                &fifo,
                "libx264",
                64,
                64,
                60,
                500,
                20,
                tx,
                CodecConfig::default(),
                Arc::new(AtomicBool::new(false)),
                stopped,
                latency,
                None,
                4,
            )
        });
        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .open(writer_path)
            .unwrap();
        writer.write_all(&vec![128; 64 * 64 * 3 / 2]).unwrap();
        let packet = rx.blocking_recv().unwrap();
        stop.request();
        task.join().unwrap().unwrap();
        packet.seq
    }

    #[test]
    fn t348_inproc_reads_native_fifo_paths() {
        use std::os::unix::ffi::OsStrExt;
        let latency = crate::latency::LatencyTracker::new();
        for name in [
            b"ordinary frames".as_slice(),
            b"native-\xff frames".as_slice(),
        ] {
            encode_one_from_fifo(latency.clone(), std::ffi::OsStr::from_bytes(name));
        }
    }

    #[test]
    fn t228_inproc_sequences_survive_restart() {
        let latency = crate::latency::LatencyTracker::new();
        let old = encode_one_from_fifo(latency.clone(), std::ffi::OsStr::new("frames"));
        let fresh = encode_one_from_fifo(latency.clone(), std::ffi::OsStr::new("frames"));
        assert_ne!(
            old, fresh,
            "T228: old and fresh frames cannot share an ACK identifier"
        );
        latency.on_rendered(fresh, 0);
        latency.on_rendered(old, 0);
    }

    #[test]
    fn t119_libx264_respects_vbv_ceiling_on_complex_frames() {
        let mut encoder = Encoder::new("libx264", 256, 144, 60, 200, 20).unwrap();
        let mut frame = vec![0; 256 * 144 * 3 / 2];
        let mut rng = 0x1234_5678u32;
        let mut bytes = 0usize;
        for _ in 0..180 {
            for pixel in &mut frame {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                *pixel = rng as u8;
            }
            for (data, _) in encoder.encode(&frame, false).unwrap() {
                bytes += data.len();
            }
        }
        // Three seconds at 200 kbit/s plus the CLI's 200 kbit minimum
        // reservoir. Allow 10% overhead for headers and encoder rounding.
        assert!(bytes > 1000);
        assert!(
            bytes * 8 <= (200_000 * 3 + 200_000) * 11 / 10,
            "libx264 ignored its rate ceiling: {} bits",
            bytes * 8
        );
    }
}
