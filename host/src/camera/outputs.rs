//! Persistent V4L2 producers. A stopped/stalled camera becomes a black frame.
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    sync::watch,
    time::Instant,
};
use uscreen_config::camera::CameraOptions;

pub type Frames = watch::Sender<Option<Arc<Vec<u8>>>>;

pub fn open_device(path: &Path, label: &str) -> Result<File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| {
            format!(
                "Open {}: install/load v4l2loopback and allow webcam access; see docs/cameras.md",
                path.display()
            )
        })?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.file_type().is_char_device(),
        "camera output must be a character device"
    );
    let name = format!(
        "/sys/dev/char/{}:{}/name",
        libc::major(metadata.rdev()),
        libc::minor(metadata.rdev())
    );
    ensure!(
        std::fs::read_to_string(name)?.trim() == label,
        "camera output must be labelled {label}"
    );
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    ensure!(
        locked == 0,
        "camera output is already owned by another UScreen camera process"
    );
    Ok(file)
}

pub fn blank(options: &CameraOptions) -> Vec<u8> {
    let pixels = (options.width * options.height) as usize;
    let mut frame = vec![128; options.frame_bytes()];
    frame[..pixels].fill(16);
    frame
}

pub fn producer(ffmpeg: &Path, device: &Path, options: &CameraOptions) -> Command {
    let mut command = Command::new(ffmpeg);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "yuv420p",
            "-video_size",
            &format!("{}x{}", options.width, options.height),
            "-framerate",
            &options.fps.to_string(),
            "-i",
            "pipe:0",
            "-an",
            "-c:v",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
            "-threads",
            "1",
            "-f",
            "v4l2",
        ])
        .arg(device)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true);
    command
}

pub async fn feed(
    child: &mut Child,
    mut frames: watch::Receiver<Option<Arc<Vec<u8>>>>,
    options: &CameraOptions,
) -> Result<()> {
    let mut input = child.stdin.take().context("camera output stdin missing")?;
    let black = blank(options);
    let result = write_frames(child, &mut input, &mut frames, &black).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), input.write_all(&black)).await;
    result
}

async fn write_frames(
    child: &mut Child,
    input: &mut tokio::process::ChildStdin,
    frames: &mut watch::Receiver<Option<Arc<Vec<u8>>>>,
    black: &[u8],
) -> Result<()> {
    let mut last_frame = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            change = frames.changed() => { change.context("camera outputs stopped")?; last_frame = Instant::now(); }
            _ = tick.tick() => {}
            status = child.wait() => { anyhow::bail!("camera output exited: {}", status?); }
        }
        let newest = frames.borrow_and_update().clone();
        let bytes = current_frame(newest.as_deref(), black, last_frame.elapsed());
        tokio::time::timeout(Duration::from_secs(2), input.write_all(bytes))
            .await
            .context("camera output stalled")??;
    }
}

fn current_frame<'a>(frame: Option<&'a Vec<u8>>, black: &'a [u8], age: Duration) -> &'a [u8] {
    match frame {
        Some(bytes) if age < Duration::from_secs(2) => bytes,
        _ => black,
    }
}

pub async fn retire(child: &mut Child) {
    if tokio::time::timeout(Duration::from_secs(1), child.wait())
        .await
        .is_ok()
    {
        return;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t539_inactive_and_stale_outputs_never_replay_camera_frames() {
        let black = [16, 128, 128];
        let live = vec![99, 99, 99];
        assert_eq!(current_frame(Some(&live), &black, Duration::ZERO), &live);
        assert_eq!(
            current_frame(Some(&live), &black, Duration::from_secs(2)),
            &black
        );
        assert_eq!(current_frame(None, &black, Duration::ZERO), &black);
        let root = tempfile::tempdir().unwrap();
        assert!(open_device(&root.path().join("missing"), "UScreen Front").is_err());
        let path = root.path().join("regular");
        std::fs::write(&path, "").unwrap();
        assert!(open_device(&path, "UScreen Front").is_err());
        assert!(open_device(Path::new("/dev/null"), "UScreen Front").is_err());
    }
}
