//! T497: calibrate stock software encoding; reject hardware before any device access.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const TEST: &str = "selection::worker::coverage_tests::t497_calibration_respects_geometry_peer_support_and_cancellation";
const FFMPEG: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/ffmpeg-calls"
previous=''
for argument do
  if [ "$previous" = '-c:v' ] && [ "$argument" != 'libx264' ]; then exit 17; fi
  previous=$argument
done
exec "$BLENT_T497_STOCK_FFMPEG" "$@"
"#;

fn command_path(name: &str) -> std::path::PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|root| root.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("T497 requires installed {name}"))
}

fn isolated() {
    let root = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let ffmpeg = command_path("ffmpeg");
    std::os::unix::fs::symlink(command_path("ffprobe"), root.path().join("ffprobe")).unwrap();
    let wrapper = root.path().join("ffmpeg");
    std::fs::write(&wrapper, FFMPEG).unwrap();
    std::fs::set_permissions(wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env("BLENT_T497_STOCK_FFMPEG", ffmpeg)
        .env("HOME", root.path())
        .env("XDG_RUNTIME_DIR", root.path())
        .env("PATH", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn snapshot() -> EncoderSettings {
    let mut settings = tests::settings();
    settings.width = 128;
    settings.height = 128;
    settings.stream_scale = 2;
    let caps = settings.decoders.as_mut().unwrap();
    caps.width = 64;
    caps.height = 64;
    caps.codecs = vec!["h264".into()];
    caps.hardware.clear();
    settings
}

async fn calibration_contract(root: &Path, base: &CaptureConfig, snapshot: &EncoderSettings) {
    let configured = probe_config(base, snapshot, "libx264");
    assert_eq!(
        (configured.width, configured.height, configured.stream_scale),
        (64, 64, 1)
    );
    assert_eq!(configured.fps, snapshot.fps);
    assert_eq!(configured.quality, snapshot.quality);
    assert_eq!(configured.bitrate, snapshot.bitrate);
    assert_eq!(configured.instance, u32::MAX);
    let candidates = calibrate(base, snapshot).await;
    assert_eq!(candidates.len(), 1, "{}", crate::test_logging::text());
    assert_eq!(candidates[0].measurement.encoder, "libx264");
    assert!(candidates[0].measurement.quality_db.unwrap() > 0.0);
    assert!(!candidates[0].hardware);
    assert!(!root.join("blent/capture.fifo").exists());
    let calls = std::fs::read_to_string(root.join("ffmpeg-calls")).unwrap();
    assert!(calls.contains("-s 64x64"));
    assert!(!calls.contains("-c:v libaom-av1"));
    assert!(!calls.contains("-c:v libvpx-vp9"));
}

async fn wait_reason(updates: &mut watch::Receiver<EncoderSettings>, reason: &str) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if updates
                .borrow_and_update()
                .selection_reason()
                .contains(reason)
            {
                return;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .expect("T497 selection transition did not arrive");
}

async fn cancellation_contract(base: CaptureConfig, snapshot: EncoderSettings) {
    let (settings, mut updates) = watch::channel(snapshot);
    let (display, visible) = watch::channel(true);
    let (stop, stopped) = watch::channel(false);
    let attachment = crate::attachment::Attachment::new(settings.clone());
    let task = spawn(
        base,
        settings.clone(),
        visible,
        stopped,
        LatencyTracker::new(),
        attachment,
    );
    wait_reason(&mut updates, "awaiting render ACKs").await;
    display.send(false).unwrap();
    wait_reason(&mut updates, "Cancelled trial").await;
    assert_eq!(settings.borrow().effective_encoder(), "libx264");
    display.send(true).unwrap();
    wait_reason(&mut updates, "awaiting render ACKs").await;
    wait_reason(&mut updates, "Preserved fallback").await;
    assert!(!settings.borrow().selection.as_ref().unwrap().verified);
    display.send(true).unwrap();
    settings.send_modify(|state| state.width_mm += 1);
    tokio::task::yield_now().await;
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn t497_calibration_respects_geometry_peer_support_and_cancellation() {
    if std::env::var_os("BLENT_T497_STOCK_FFMPEG").is_none() {
        isolated();
        return;
    }
    crate::test_logging::enable();
    let root = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    let base = CaptureConfig {
        instance: u32::MAX,
        helper_path: root.join("forbidden-helper"),
        vaapi_device: root
            .join("forbidden-render-node")
            .to_string_lossy()
            .into_owned(),
        ..Default::default()
    };
    let snapshot = snapshot();
    calibration_contract(&root, &base, &snapshot).await;
    cancellation_contract(base, snapshot).await;
    assert!(crate::test_logging::text().contains("Automatic encoder probe rejected candidate"));
}
