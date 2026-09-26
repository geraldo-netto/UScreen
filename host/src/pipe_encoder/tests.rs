use super::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

fn settings() -> Settings {
    Settings {
        encoder: "libx264".into(),
        vaapi_device: String::new(),
        fps: 30,
        bitrate: 4000,
        quality: 18,
        workers: 1,
        adaptive_idle: false,
    }
}

fn fixture(role: &str, root: &std::path::Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "pipe_encoder::tests::t529_encoder_fixture",
            "--nocapture",
        ])
        .env("BLENT_T529_ENCODER_ROLE", role)
        .env("BLENT_T529_ENCODER_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

#[test]
fn t529_encoder_fixture() {
    use std::io::Read;
    let Ok(role) = std::env::var("BLENT_T529_ENCODER_ROLE") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("BLENT_T529_ENCODER_ROOT").unwrap());
    std::fs::write(root.join("ready"), b"ready").unwrap();
    match role.as_str() {
        "stall" => {
            let mut prefix = [0; 2];
            std::io::stdin().read_exact(&mut prefix).unwrap();
            std::fs::write(root.join("partial"), prefix).unwrap();
            std::thread::sleep(Duration::from_secs(2));
            std::fs::write(root.join("escaped"), b"escaped").unwrap();
        }
        "exit" => std::process::exit(7),
        "discard" => {
            let mut prefix = [0; 2];
            std::io::stdin().read_exact(&mut prefix).unwrap();
        }
        "drain" => {
            std::io::copy(&mut std::io::stdin(), &mut std::io::sink()).unwrap();
        }
        _ => panic!("T529: unknown encoder role"),
    }
    std::process::exit(0);
}

async fn ready(root: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !root.join("ready").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

// Independent test reader: verify stock FFmpeg's metadata+payload framing and
// send precisely those unchanged encoded bytes into its ordinary H.264 decoder.
async fn encoded_packets(output: ChildStdout) -> Vec<Vec<u8>> {
    let mut reader = BufReader::new(output);
    let mut packets = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap() == 0 {
            break;
        }
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.trim().split(',').map(str::trim).collect();
        let size: usize = fields[4].parse().unwrap();
        assert!((1..=crate::video_queue::MAX_FRAME_BYTES).contains(&size));
        let mut packet = vec![0; size];
        reader.read_exact(&mut packet).await.unwrap();
        let checksum = u32::from_str_radix(fields[5].trim_start_matches("0x"), 16).unwrap();
        let mut adler = simd_adler32::Adler32::from_checksum(0);
        adler.write(&packet);
        assert_eq!(adler.finish(), checksum);
        packets.push(packet);
    }
    packets
}

#[tokio::test]
async fn t529_stock_software_encoder_round_trips_fresh_sessions_and_geometry() {
    use blent_config::commands::AsyncCommandExt;
    for dimensions in [(64, 64), (128, 64)] {
        let Encoder { mut input, output } = Encoder::start(
            OsStr::new("ffmpeg"),
            &settings(),
            dimensions,
            Duration::from_secs(5),
        )
        .unwrap();
        let drain = tokio::spawn(encoded_packets(output));
        let pixels = (dimensions.0 * dimensions.1) as usize;
        let mut frame = vec![128; pixels * 3 / 2];
        frame[..pixels].fill(64);
        assert!(input
            .write_frame((dimensions.0 + 2, dimensions.1), &frame)
            .await
            .is_err());
        assert!(input
            .write_frame(dimensions, &frame[..frame.len() - 1])
            .await
            .is_err());
        for _ in 0..3 {
            input.write_frame(dimensions, &frame).await.unwrap();
        }
        input.shutdown().await.unwrap();
        let packets = drain.await.unwrap();
        assert_eq!(packets.len(), 3);
        let decoded = Command::new("ffmpeg")
            .args([
                "-v", "error", "-f", "h264", "-i", "pipe:0", "-pix_fmt", "nv12", "-f", "rawvideo",
                "pipe:1",
            ])
            .output_input_timeout(Some(&packets.concat()), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        assert_eq!(decoded.stdout.len(), frame.len() * 3);
        for decoded in decoded.stdout.chunks_exact(frame.len()) {
            assert!(decoded
                .iter()
                .zip(&frame)
                .all(|(&a, &b)| a.abs_diff(b) <= 2));
        }
    }
}

#[tokio::test]
async fn t529_partial_pipe_timeout_and_cancellation_retire_encoder() {
    for cancel in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut encoder = Encoder::spawn(
            &mut fixture("stall", root.path()),
            (512, 512),
            Duration::from_millis(200),
        )
        .unwrap();
        ready(root.path()).await;
        let frame = vec![128; 512 * 512 * 3 / 2];
        // Windows' asynchronous pipe can accept an entire pending buffer before
        // backpressure. Bound the attempt count; require retirement once the
        // stagnant reader prevents progress, independent of native pipe size.
        let blocked = async {
            for _ in 0..4 {
                encoder.input.write_frame((512, 512), &frame).await?;
            }
            panic!("T529: stagnant reader accepted every bounded frame");
            #[allow(unreachable_code)]
            Ok::<_, io::Error>(())
        };
        if cancel {
            assert!(tokio::time::timeout(Duration::from_millis(100), blocked)
                .await
                .is_err());
        } else {
            assert_eq!(blocked.await.unwrap_err().kind(), io::ErrorKind::TimedOut);
        }
        assert_eq!(
            std::fs::read(root.path().join("partial")).unwrap(),
            [128, 128]
        );
        assert_eq!(
            encoder
                .input
                .write_frame((512, 512), &frame)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(!root.path().join("escaped").exists());
    }
}

#[tokio::test]
async fn t529_encoder_exit_start_failure_and_missing_pipes_are_errors() {
    let root = tempfile::tempdir().unwrap();
    assert!(Encoder::start(
        root.path().join("missing").as_os_str(),
        &settings(),
        (64, 64),
        Duration::from_secs(1)
    )
    .is_err());
    for missing_input in [true, false] {
        let mut command = fixture("drain", root.path());
        if missing_input {
            command.stdin(Stdio::null());
        } else {
            command.stdout(Stdio::null());
        }
        assert!(Encoder::spawn(&mut command, (64, 64), Duration::from_secs(1)).is_err());
    }
    let mut encoder = Encoder::spawn(
        &mut fixture("exit", root.path()),
        (64, 64),
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(encoder.input.shutdown().await.is_err());
    assert!(encoder.input.shutdown().await.is_ok());
}

#[tokio::test]
async fn t529_owned_input_flush_and_shutdown_are_repeatable() {
    let root = tempfile::tempdir().unwrap();
    let mut child = OwnedChild::spawn(&mut fixture("drain", root.path())).unwrap();
    let mut input = OwnedInput {
        stdin: child.take_stdin(),
        child: Some(child),
        finishing: None,
        timeout: Duration::from_secs(2),
    };
    input.write_all(b"frame bytes").await.unwrap();
    input.flush().await.unwrap();
    input.shutdown().await.unwrap();
    input.flush().await.unwrap();
    input.shutdown().await.unwrap();
    assert_eq!(
        input.write_all(b"late").await.unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[tokio::test]
async fn t529_shutdown_reports_an_unfinished_native_write_even_after_zero_exit() {
    let root = tempfile::tempdir().unwrap();
    let mut encoder = Encoder::spawn(
        &mut fixture("discard", root.path()),
        (512, 512),
        Duration::from_secs(2),
    )
    .unwrap();
    ready(root.path()).await;
    let written = encoder
        .input
        .write_frame((512, 512), &vec![128; 512 * 512 * 3 / 2])
        .await;
    let finished = encoder.input.shutdown().await;
    assert!(
        written.and(finished).is_err(),
        "T529: a zero exit concealed an unfinished frame write"
    );
}

#[test]
fn t529_invalid_encoder_settings_are_rejected_before_launch() {
    let mut spec = settings();
    assert!(validate(&spec, (64, 64), Duration::from_secs(1)).is_ok());
    for value in [0, 1, 3, 4097, u32::MAX] {
        assert!(validate(&spec, (value, 64), Duration::from_secs(1)).is_err());
    }
    for value in [Duration::ZERO, Duration::from_secs(61), Duration::MAX] {
        assert!(validate(&spec, (64, 64), value).is_err());
    }
    for value in [0, 91, u32::MAX] {
        spec.fps = value;
        assert!(validate(&spec, (64, 64), Duration::from_secs(1)).is_err());
    }
    spec = settings();
    for value in [0, 999, 60001, u32::MAX] {
        spec.bitrate = value;
        assert!(validate(&spec, (64, 64), Duration::from_secs(1)).is_err());
    }
    spec = settings();
    for value in [0, 11, 33, u32::MAX] {
        spec.quality = value;
        assert!(validate(&spec, (64, 64), Duration::from_secs(1)).is_err());
    }
    spec = settings();
    spec.workers = 129;
    assert!(validate(&spec, (64, 64), Duration::from_secs(1)).is_err());
    spec = settings();
    spec.encoder = "h264_nvenc".into();
    assert!(validate(&spec, (64, 64), Duration::from_secs(1)).is_err());
}
