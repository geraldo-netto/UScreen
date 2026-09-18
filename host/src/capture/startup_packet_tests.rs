//! T447: rawvideo probing must retain the first supplied picture.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn command(encoder: &str) -> Command {
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
    let mut cmd = Command::new("ffmpeg");
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    cmd
}

async fn wait_first(encoder: &str, output: tokio::process::ChildStdout) {
    let codec = Codec::from_encoder(encoder);
    if codec.framed() {
        let mut input = tokio::io::BufReader::new(output);
        let mut parser = crate::ivf::IvfPacketizer::new(codec, Default::default());
        let packet = parser.read_from(&mut input).await.unwrap().1;
        assert_eq!(packet.len(), 1);
        assert!(packet[0].is_idr);
    } else {
        let mut input = output.take(1024 * 1024);
        let mut bytes = Vec::new();
        loop {
            assert!(
                input.read_buf(&mut bytes).await.unwrap() > 0,
                "T447: no initial picture"
            );
            if crate::encoder_io::annex_b_offsets(&bytes)
                .any(|(_, header)| bytes.get(header).is_some_and(|value| value & 0x1f == 5))
            {
                break;
            }
        }
    }
}

async fn first_picture(encoder: &str) {
    let mut child = command(encoder).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(&vec![80; 64 * 64 * 3 / 2]).await.unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        wait_first(encoder, child.stdout.take().unwrap()),
    )
    .await;
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    assert!(
        result.is_ok(),
        "T447: {encoder} waited for a second picture or EOF"
    );
}

#[tokio::test]
async fn t447_h264_encoder_outputs_first_picture_with_stdin_open() {
    first_picture("libx264").await;
}

#[tokio::test]
async fn t447_vp9_outputs_first_picture_with_stdin_open() {
    first_picture("libvpx-vp9").await;
}

#[tokio::test]
async fn t447_av1_outputs_first_picture_with_stdin_open() {
    first_picture("libaom-av1").await;
}

#[tokio::test]
async fn t447_all_supplied_raw_pictures_survive_startup() {
    for encoder in ["libx264", "libvpx-vp9", "libaom-av1"] {
        let mut child = command(encoder).spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        input
            .write_all(&vec![80; 5 * 64 * 64 * 3 / 2])
            .await
            .unwrap();
        input.shutdown().await.unwrap();
        drop(input);
        let mut output = tokio::io::BufReader::new(child.stdout.take().unwrap());
        let mut parser = Packetizer::new(Codec::from_encoder(encoder), Default::default());
        let received = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut count = 0;
            loop {
                let (bytes, packets) = parser.read_from(&mut output).await.unwrap();
                count += packets.len();
                if bytes == 0 {
                    break count;
                }
            }
        })
        .await
        .unwrap();
        assert!(child.wait().await.unwrap().success());
        assert_eq!(received, 5, "T447: {encoder} discarded a probe picture");
    }
}
