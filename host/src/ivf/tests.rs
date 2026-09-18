use super::*;

fn header() -> Vec<u8> {
    b"DKIF\0\0\x20\0VP90\x40\0\x40\0\x1e\0\0\0\x01\0\0\0\x02\0\0\0\0\0\0\0".to_vec()
}
fn fixture() -> Vec<u8> {
    let mut data = header();
    append_packet(&mut data, &[0x82, 0x49, 0x83, 0x42, 1, 2], 0);
    append_packet(&mut data, &[0x86, 3, 4], 1);
    data
}
fn append_packet(data: &mut Vec<u8>, payload: &[u8], seq: u32) {
    data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    data.extend_from_slice(&u64::from(seq).to_le_bytes());
    data.extend_from_slice(payload);
}
async fn packets(data: &[u8]) -> Result<Vec<VideoPacket>> {
    packets_for(Codec::Vp9, data).await
}
async fn packets_for(codec: Codec, data: &[u8]) -> Result<Vec<VideoPacket>> {
    let mut parser = IvfPacketizer::new(codec, Default::default());
    let mut input = data;
    let mut packets = Vec::new();
    loop {
        let (read, batch) = parser.read_from(&mut input).await?;
        packets.extend(batch);
        if read == 0 {
            return Ok(packets);
        }
    }
}

#[tokio::test]
async fn t433_stock_av1_preserves_decode_hashes_and_late_join_points() {
    let profile = uscreen_config::encoding::Profile::new("libaom-av1", 30, 20000, 18).unwrap();
    let encoded = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x64:rate=30",
            "-frames:v",
            "6",
            "-c:v",
            "libaom-av1",
        ])
        .args(
            profile
                .cli_options(false)
                .into_iter()
                .flat_map(|(key, value)| [key, value]),
        )
        .args(["-g", "3", "-f", "ivf", "pipe:1"])
        .output()
        .expect("T433: stock ffmpeg required");
    assert!(
        encoded.status.success(),
        "{}",
        String::from_utf8_lossy(&encoded.stderr)
    );
    let frames = packets_for(Codec::Av1, &encoded.stdout).await.unwrap();
    assert_eq!(frames.len(), 6);
    assert_eq!(
        frames.iter().map(|f| f.is_idr).collect::<Vec<_>>(),
        [true, false, false, true, false, false]
    );
    assert_eq!(
        frames[0].codec_config.as_ref().unwrap().as_ref(),
        b"USC1\x04\0\0\0\x40\0\0\0\x40"
    );
    let reference = frame_hashes(&encoded.stdout);
    assert_eq!(reference.len(), 6);
    assert_eq!(frame_hashes(&av1_ivf(&frames)), reference);
    let joined = av1_ivf(&frames[3..]);
    assert_eq!(frame_hashes(&joined), reference[3..]);
    assert!(packets_for(Codec::Av1, &joined).await.unwrap()[0].is_idr);
    assert!(packets_for(Codec::Vp9, &encoded.stdout).await.is_err());
    for missing in 1..=12 {
        assert!(packets_for(
            Codec::Av1,
            &encoded.stdout[..encoded.stdout.len() - missing]
        )
        .await
        .is_err());
    }
}

fn av1_ivf(frames: &[VideoPacket]) -> Vec<u8> {
    let mut data = header();
    data[8..12].copy_from_slice(b"AV01");
    data[24..28].copy_from_slice(&(frames.len() as u32).to_le_bytes());
    for (index, frame) in frames.iter().enumerate() {
        append_packet(&mut data, &frame.data, index as u32);
    }
    data
}

fn frame_hashes(data: &[u8]) -> Vec<String> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(data).unwrap();
    let decoded = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(file.path())
        .args(["-f", "framemd5", "pipe:1"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    String::from_utf8_lossy(&decoded.stdout)
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.rsplit(',').next().unwrap().trim().to_string())
        .collect()
}
#[tokio::test]
async fn t432_ivf_preserves_packet_flags_configuration_and_retirement() {
    let packets = packets(&fixture()).await.unwrap();
    assert_eq!(packets.len(), 2);
    assert!(packets[0].is_idr);
    assert!(!packets[1].is_idr);
    assert_eq!(packets[0].data.as_ref(), &[0x82, 0x49, 0x83, 0x42, 1, 2]);
    assert_eq!(packets[1].seq, packets[0].seq + 1);
    assert_eq!(
        packets[0].codec_config.as_ref().unwrap().as_ref(),
        b"USC1\x03\0\0\0\x40\0\0\0\x40"
    );
    assert!(!packets[0]
        .generation
        .load(std::sync::atomic::Ordering::Acquire));
}
#[tokio::test]
async fn t432_ivf_fragmentation_and_eof_do_not_drop_or_join_frames() {
    use tokio::io::AsyncWriteExt;
    let (mut writer, mut reader) = tokio::io::duplex(1);
    let task = tokio::spawn(async move {
        for byte in fixture() {
            writer.write_all(&[byte]).await.unwrap();
        }
    });
    let mut parser = IvfPacketizer::new(Codec::Vp9, Default::default());
    assert!(parser.read_from(&mut reader).await.unwrap().1[0].is_idr);
    assert_eq!(
        parser.read_from(&mut reader).await.unwrap().1[0]
            .data
            .as_ref(),
        &[0x86, 3, 4]
    );
    assert_eq!(parser.read_from(&mut reader).await.unwrap().0, 0);
    task.await.unwrap();
}
#[tokio::test]
async fn t432_ivf_rejects_truncation_bad_codec_and_sizes() {
    let data = fixture();
    for missing in 1..=14 {
        assert!(packets(&data[..data.len() - missing]).await.is_err());
    }
    for (offset, value) in [(8, b'X'), (4, 1), (32, 255), (44, 0), (45, 0)] {
        let mut corrupt = data.clone();
        corrupt[offset] = value;
        assert!(packets(&corrupt).await.is_err());
    }
    let mut oversized = header();
    oversized.extend_from_slice(&u32::MAX.to_le_bytes());
    oversized.extend_from_slice(&[0; 8]);
    assert!(packets(&oversized).await.is_err());
    assert!(!vp9_keyframe(&[0x88]).unwrap());
}
#[tokio::test]
async fn t432_stock_vp9_ivf_roundtrip() {
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x64:rate=30",
            "-frames:v",
            "6",
            "-c:v",
            "libvpx-vp9",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-lag-in-frames",
            "0",
            "-auto-alt-ref",
            "0",
            "-g",
            "3",
            "-f",
            "ivf",
            "-flush_packets",
            "1",
            "pipe:1",
        ])
        .output()
        .expect("T432: stock ffmpeg required");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let packets = packets(&output.stdout).await.unwrap();
    assert_eq!(packets.len(), 6);
    assert!(packets[0].is_idr && packets[3].is_idr);
    assert!(!packets[1].is_idr && !packets[4].is_idr);
    decode_ivf(&packets);
}
fn decode_ivf(packets: &[VideoPacket]) {
    use std::io::Write;
    let mut ivf = tempfile::NamedTempFile::new().unwrap();
    let mut data = header();
    data[24..28].copy_from_slice(&(packets.len() as u32).to_le_bytes());
    for packet in packets {
        append_packet(&mut data, &packet.data, packet.seq);
    }
    ivf.write_all(&data).unwrap();
    let output = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(ivf.path())
        .args(["-f", "framemd5", "pipe:1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let frames = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with('#'))
        .count();
    assert_eq!(
        frames,
        packets.len(),
        "T432: frame payloads must decode completely"
    );
}
