//! T421: duplicate/backward input PTS must not lose frames or wall-time IDRs.
use super::*;
use blent_config::commands::AsyncCommandExt;

const SOURCE: &str = "nullsrc=size=64x64:rate=60,geq=lum='16+4*N':cb=128:cr=128,format=nv12,settb=1/1000000,setpts='if(lt(N,10),0,if(lt(N,20),1100000,if(lt(N,30),500000,2200000)))'";

async fn encode(path: &std::path::Path, encoder: &str) -> std::process::Output {
    let config = CaptureConfig {
        encoder: encoder.into(),
        fps: 60,
        instance: u32::MAX,
        ..Default::default()
    };
    let production = CliEncoder { config: &config }
        .encoder_command(64, 64, false)
        .unwrap();
    let args = production
        .as_std()
        .get_args()
        .map(std::ffi::OsStr::to_owned)
        .collect::<Vec<_>>();
    let start = args.iter().position(|arg| arg == "-i").unwrap() + 2;
    let mut output = args[start..args.len() - 3].to_vec();
    // T588: this fixture measures timestamps and exact picture identity. Older
    // libvpx versions quantize even these flat markers beyond the two-level
    // bound; retain production timing options but make VP9 markers lossless.
    if encoder == "libvpx-vp9" {
        output.extend(["-lossless", "1", "-crf", "0"].map(Into::into));
    }
    output.extend(["-f".into(), "nut".into(), path.as_os_str().to_owned()]);
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-f",
            "lavfi",
            "-i",
            SOURCE,
            "-frames:v",
            "40",
        ])
        .args(output)
        .output_bounded()
        .await
        .unwrap()
}

async fn packets(path: &std::path::Path) -> Vec<serde_json::Value> {
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "packet=pts_time,dts_time,flags",
            "-of",
            "json",
        ])
        .arg(path)
        .output_bounded()
        .await
        .unwrap();
    assert!(probe.status.success());
    serde_json::from_slice::<serde_json::Value>(&probe.stdout).unwrap()["packets"]
        .as_array()
        .unwrap()
        .clone()
}

async fn verify_frames(path: &std::path::Path) {
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "nv12",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output_bounded()
        .await
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let frame_bytes = 64 * 64 * 3 / 2;
    assert_eq!(
        decoded.stdout.len(),
        40 * frame_bytes,
        "T421: pictures lost or duplicated"
    );
    for (index, frame) in decoded.stdout.chunks_exact(frame_bytes).enumerate() {
        let expected = 16 + index as i32 * 4;
        assert!(
            (i32::from(frame[0]) - expected).abs() <= 2,
            "T421: picture order/content changed at {index}: {} vs {expected}",
            frame[0]
        );
    }
}

#[tokio::test]
async fn t421_stock_cli_preserves_pictures_and_wall_time_keys_with_monotonic_timestamps() {
    verify_stream("libx264").await;
}

#[tokio::test]
async fn t421_vp9_and_av1_keep_monotonic_picture_timing() {
    for encoder in ["libvpx-vp9", "libaom-av1"] {
        verify_stream(encoder).await;
    }
}

async fn verify_stream(encoder: &str) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("timestamps.nut");
    let result = encode(&path, encoder).await;
    let log = String::from_utf8_lossy(&result.stderr).to_lowercase();
    assert!(result.status.success(), "T421: {log}");
    assert!(
        !log.contains("non-monoton") && !log.contains("non monoton"),
        "T421: {log}"
    );
    let packets = packets(&path).await;
    assert_eq!(packets.len(), 40);
    let timestamps = packets
        .iter()
        .map(|p| p["dts_time"].as_str().unwrap().parse::<f64>().unwrap())
        .collect::<Vec<_>>();
    assert!(timestamps.windows(2).all(|pair| pair[1] > pair[0]));
    let keys = packets
        .iter()
        .filter(|p| p["flags"].as_str().unwrap().contains('K'))
        .map(|p| p["pts_time"].as_str().unwrap().parse::<f64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [0.0, 1.1, 2.2],
        "T421: sparse wall-time keyframe schedule changed"
    );
    verify_frames(&path).await;
}

// T588: timing markers must survive the fixture codec exactly. Production
// lossy quality is covered by encoder-option tests, not a two-level pixel bound.
#[tokio::test]
async fn t588_vp9_timing_fixture_preserves_every_luma_sample() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("lossless-timing.nut");
    assert!(encode(&path, "libvpx-vp9").await.status.success());
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "nv12",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output_bounded()
        .await
        .unwrap();
    assert!(decoded.status.success());
    assert_eq!(decoded.stdout.len(), 40 * 64 * 64 * 3 / 2);
    for (index, frame) in decoded.stdout.chunks_exact(64 * 64 * 3 / 2).enumerate() {
        assert!(
            frame[..64 * 64]
                .iter()
                .all(|&value| value == 16 + index as u8 * 4),
            "T588: lossy encoding corrupted timing marker {index}"
        );
        assert!(frame[64 * 64..].iter().all(|&value| value == 128));
    }
}
