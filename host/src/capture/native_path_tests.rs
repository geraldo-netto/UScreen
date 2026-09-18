use super::*;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt};
use tokio::process::Command;

#[tokio::test]
async fn t348_native_runtime_paths_agree_across_capture_resources() {
    const NAME: &str =
        "capture::native_path_tests::t348_native_runtime_paths_agree_across_capture_resources";
    if std::env::var_os("USCREEN_T348_CHILD").is_none() {
        for name in [
            b"ordinary space [1]".as_slice(),
            b"native-\xff space [1]".as_slice(),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let runtime = dir.path().join(std::ffi::OsString::from_vec(name.to_vec()));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&runtime)
                .unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", NAME, "--nocapture"])
                .env("USCREEN_T348_CHILD", "1")
                .env("XDG_RUNTIME_DIR", &runtime)
                .env("HOME", dir.path())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "T348: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        return;
    }
    native_capture_resources().await;
}

async fn native_capture_resources() {
    let expected = crate::runtime::fifo_path_for(0).unwrap();
    let path = fifo_path_for(0).unwrap();
    assert_eq!(
        std::path::Path::new(&path),
        expected,
        "T348: lossy runtime FIFO"
    );
    assert_eq!(
        crate::runtime::runtime_dir().unwrap(),
        expected.parent().unwrap()
    );
    crate::runtime::new_session_token().unwrap();
    assert!(expected.parent().unwrap().join("token").is_file());
    let missing = expected.parent().unwrap().join("missing").join("frames");
    let error = helper::ensure_fifo(&missing).unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .raw_os_error(),
        Some(libc::ENOENT)
    );
    helper::ensure_fifo(&path).unwrap();
    assert!(std::fs::metadata(&expected).unwrap().file_type().is_fifo());
    let mut manager = CaptureManager::new(CaptureConfig {
        encoder: "libx264".into(),
        edid_path: Some("unused-fixture-edid".into()),
        ..Default::default()
    });
    // Cleanup belongs to the manager only after it creates the native FIFO.
    manager.helper.fifo = Some(fifo::Owned::create(&path).unwrap());
    assert_native_argument(
        &manager.helper.command(&manager.config, &path).unwrap(),
        "--capture-fifo",
        &expected,
    );
    #[cfg(not(feature = "inproc-encoder"))]
    assert_native_argument(
        &cli_encoder::CliEncoder {
            config: &manager.config,
        }
        .encoder_command(640, 480, true)
        .unwrap(),
        "-i",
        &expected,
    );
    let children = tempfile::tempdir().unwrap();
    super::orphan_tests::fixture_programs(children.path());
    let mut child =
        super::orphan_tests::fixture_child(&children.path().join("ffmpeg"), "-i", &expected, false)
            .await;
    assert_eq!(
        crate::doctor::encoders_for_fifo(
            &uscreen_config::linux::processes::same_user_processes().unwrap(),
            &expected
        ),
        vec![child.id().unwrap()]
    );
    process::retire_orphan_capture(&path).await.unwrap();
    assert!(crate::doctor::encoders_for_fifo(
        &uscreen_config::linux::processes::same_user_processes().unwrap(),
        &expected
    )
    .is_empty());
    assert!(
        child.try_wait().unwrap().is_some(),
        "T348: native-path orphan survived"
    );
    manager.shutdown().await;
    assert!(!expected.exists(), "T348: native FIFO survived cleanup");
}

fn assert_native_argument(command: &Command, flag: &str, path: &std::path::Path) {
    let args = command.as_std().get_args().collect::<Vec<_>>();
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == flag && pair[1] == path.as_os_str()),
        "T348: native argument lost: {args:?}"
    );
}
