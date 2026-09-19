//! T497: metadata and launch preparation require no DRM device.
use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn t497_helper_command_prepares_private_edid_and_respects_card_priority() {
    let Ok(root) = std::env::var("USCREEN_T497_HELPER_ROOT") else {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "capture::helper::coverage_tests::t497_helper_command_prepares_private_edid_and_respects_card_priority", "--nocapture"])
            .env("USCREEN_T497_HELPER_ROOT", directory.path())
            .env("HOME", directory.path()).env("XDG_RUNTIME_DIR", directory.path()).output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    };
    let mut helper = HelperProcess::new();
    helper.card = Some(17);
    let mut config = CaptureConfig {
        helper_path: "/never/execute/helper".into(),
        width: 640,
        height: 480,
        fps: 120,
        conversion_threads: 128,
        stream_scale: 2,
        ..Default::default()
    };
    let fifo = Path::new(&root).join("unused.fifo");
    let arguments = |config: &CaptureConfig| {
        helper
            .command(config, &fifo)
            .unwrap()
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    let args = arguments(&config);
    assert_eq!(
        &args[2..8],
        [
            "--fps",
            "120",
            "--conversion-threads",
            "128",
            "--scale",
            "2"
        ]
    );
    assert_eq!(&args[args.len() - 2..], ["--preferred-card", "17"]);
    let edid = std::fs::read(&args[1]).unwrap();
    assert_eq!(edid.len(), 128);
    assert_eq!(
        edid.iter().fold(0u8, |sum, value| sum.wrapping_add(*value)),
        0
    );
    assert_eq!(
        Path::new(&args[1]),
        crate::edid::ensure_edid(640, 480, 90).unwrap()
    );
    config.card = Some(23);
    let args = arguments(&config);
    assert_eq!(&args[args.len() - 2..], ["--card", "23"]);
    assert!(!fifo.exists(), "T497 command construction started capture");
}

#[test]
fn t497_malformed_helper_reports_never_replace_the_last_valid_geometry() {
    let helper = HelperProcess::new();
    HelperProcess::publish_helper_line("STREAM_SIZE 640 480", &helper.mode_tx, &helper.stream_tx);
    HelperProcess::publish_helper_line(
        "MODE_CHANGED 1280 960 60",
        &helper.mode_tx,
        &helper.stream_tx,
    );
    for token in [
        "",
        "0",
        "-1",
        "4294967296",
        "NaN",
        "999999999999999999999",
        "x",
    ] {
        for line in [
            format!("STREAM_SIZE {token} 480"),
            format!("STREAM_SIZE 640 {token}"),
            format!("MODE_CHANGED {token} 960 60"),
            format!("MODE_CHANGED 1280 {token} 60"),
        ] {
            HelperProcess::publish_helper_line(&line, &helper.mode_tx, &helper.stream_tx);
        }
    }
    assert_eq!(*helper.stream_rx.borrow(), Some((640, 480)));
    assert_eq!(
        *helper.mode_rx.borrow(),
        Some(DetectedMode {
            width: 1280,
            height: 960,
            refresh: 60
        })
    );
}

#[tokio::test]
async fn t497_helper_handshake_reports_eof_bad_numbers_and_invalid_utf8() {
    for (script, expected) in [
        ("printf 'noise\\nEVDI_CONNECTED card17\\n'", Some(17)),
        ("printf 'EVDI_CONNECTED card-1\\n'", None),
        ("printf 'noise\\n'", None),
        ("printf '\\377\\n'", None),
    ] {
        let mut child = Command::new("/bin/sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let result = HelperProcess::await_helper_card(&mut lines).await;
        assert_eq!(result.ok(), expected, "T497 script: {script}");
        assert!(child.wait().await.unwrap().success());
    }
}
