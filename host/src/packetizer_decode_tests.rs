//! T407: the owned-input path must preserve real stock-FFmpeg bitstreams.
use super::*;
use std::process::{Command, Stdio};

fn encode(codec: Codec) -> Vec<u8> {
    let (name, flag, options) = match codec {
        Codec::H264 => ("libx264", "-x264-params", "keyint=4:scenecut=0:aud=1"),
        Codec::Vp9 => unreachable!("Annex B fixture"),
        Codec::Hevc => (
            "libx265",
            "-x265-params",
            "pools=none:frame-threads=1:keyint=4:scenecut=0:aud=1:log-level=error",
        ),
    };
    let output = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x48:rate=30",
            "-frames:v",
            "12",
            "-threads",
            "1",
            "-c:v",
            name,
            "-preset",
            "ultrafast",
            "-tune",
            "zerolatency",
            flag,
            options,
            "-f",
            codec.muxer(),
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "T407 encoder: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn decode(codec: Codec, data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-threads",
            "1",
            "-f",
            codec.muxer(),
            "-i",
            "pipe:0",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    // Drain output concurrently with writes; larger fixtures cannot deadlock.
    let data = data.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&data).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "T407 decoder: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

struct Fragments<'a> {
    data: &'a [u8],
    chunk: usize,
}
impl tokio::io::AsyncRead for Fragments<'_> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
        output: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let count = self.chunk.min(self.data.len()).min(output.remaining());
        output.put_slice(&self.data[..count]);
        self.data = &self.data[count..];
        std::task::Poll::Ready(Ok(()))
    }
}

async fn collect(codec: Codec, data: &[u8], chunk: usize) -> Vec<VideoPacket> {
    let mut parser = AnnexBPacketizer::new(codec, Default::default());
    let mut source = Fragments { data, chunk };
    let mut packets = Vec::new();
    loop {
        let (count, output) = parser.read_from(&mut source).await.unwrap();
        packets.extend(output);
        if count == 0 {
            return packets;
        }
    }
}

#[tokio::test]
async fn t407_real_h264_hevc_decode_equivalence_after_fragmented_reads_and_retirement() {
    for codec in [Codec::H264, Codec::Hevc] {
        let stream = encode(codec);
        let original_pixels = decode(codec, &stream);
        assert_eq!(original_pixels.len(), 12 * 64 * 48 * 3 / 2);
        let reference = collect(codec, &stream, 512 * 1024).await;
        for chunk in [1, 3, 7, 4093] {
            let packets = collect(codec, &stream, chunk).await;
            assert_eq!(packets.len(), 12);
            for (index, (packet, expected)) in packets.iter().zip(&reference).enumerate() {
                assert_eq!(packet.data, expected.data);
                assert_eq!(packet.codec_config, expected.codec_config);
                assert_eq!(packet.is_idr, expected.is_idr);
                assert_eq!(packet.seq, index as u32);
                assert!(!packet.generation.load(std::sync::atomic::Ordering::Acquire));
            }
            let bytes = packets
                .iter()
                .flat_map(|packet| packet.data.iter().copied())
                .collect::<Vec<_>>();
            assert_eq!(decode(codec, &bytes), original_pixels);
        }
    }
}

#[tokio::test]
async fn t407_owned_capacity_and_slow_consumers_remain_bounded() {
    // A large slice plus a tiny continuation must not double the retained AU.
    let mut data = vec![0, 0, 0, 1, 1, 0x80];
    data.resize(1024 * 1024 + 6, 0x55);
    data.extend([0, 0, 0, 1, 1, 0x40, 0x55]);
    let maximum = super::INPUT_BYTES;
    let mut parser = AnnexBPacketizer::new(Codec::H264, Default::default());
    let mut source = Fragments {
        data: &data,
        chunk: 4093,
    };
    let mut output = Vec::new();
    loop {
        let (count, packets) = parser.read_from(&mut source).await.unwrap();
        assert!(parser.buffer.capacity() <= maximum);
        assert!(parser.pending_access_unit.capacity() <= MAX_FRAME_BYTES);
        output.extend(packets);
        if count == 0 {
            break;
        }
    }
    let budget = crate::media_storage::Budget::new(data.len() + 4096);
    assert_eq!(output.len(), 1);
    assert!(output[0].data.charge(&budget));
    let delayed = output[0].data.slice(0..1);
    drop(output);
    drop(parser);
    assert!(budget.usage().0 >= data.len());
    assert_eq!(delayed.as_ref(), &[0]);
    drop(delayed);
    assert_eq!(budget.usage().0, 0);
}
