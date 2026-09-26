use super::*;
use crate::{attachment::Attachment, capture::CaptureConfig};
use blent_config::commands::SyncCommandExt;
use sha2::{Digest, Sha256};
use std::path::Path;

impl Cache {
    pub async fn open(
        base: &CaptureConfig,
        snapshot: &EncoderSettings,
        attachment: &Attachment,
    ) -> Option<std::sync::Arc<Self>> {
        if !base.profile_cache {
            return None;
        }
        let lease = attachment.lease();
        let identity = lease.profile_identity()?;
        fingerprint(&identity, "", snapshot, base)?;
        let base = base.clone();
        let snapshot = snapshot.clone();
        let cache = tokio::task::spawn_blocking(move || {
            let stamp = host_stamp(&base)?;
            let fingerprint = fingerprint(&identity, &stamp, &snapshot, &base)?;
            let path = blent_config::config_path()
                .ok()?
                .with_file_name("profile-cache.json");
            Some(Self {
                path,
                fingerprint,
                lease: Some(lease),
            })
        })
        .await
        .ok()
        .flatten()?;
        cache.active().then(|| std::sync::Arc::new(cache))
    }
}

pub(super) fn fingerprint(
    identity: &(String, &'static str),
    host: &str,
    snapshot: &EncoderSettings,
    base: &CaptureConfig,
) -> Option<String> {
    let mut caps = snapshot.decoders.clone()?;
    if snapshot.encoder != "auto"
        || !snapshot.geometry_ready
        || !caps.valid()
        || caps.protocol != 2
        || caps.software.is_none()
    {
        return None;
    }
    caps.scope = None;
    let data = serde_json::to_vec(&(
        1,
        identity,
        host,
        caps,
        super::super::super::Key::new(snapshot).format,
        (
            &base.vaapi_device,
            base.ten_bit,
            base.stream_scale,
            base.conversion_threads,
            base.encoder_workers,
            &base.edid_path,
        ),
    ))
    .ok()?;
    Some(format!("{:x}", Sha256::digest(data)))
}

fn hash_file(digest: &mut Sha256, path: &Path) -> Option<()> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut chunk = [0; 65536];
    loop {
        let count = file.read(&mut chunk).ok()?;
        if count == 0 {
            break;
        }
        digest.update(&chunk[..count]);
    }
    Some(())
}

fn host_stamp(base: &CaptureConfig) -> Option<String> {
    let mut digest = Sha256::new();
    let ffmpeg = blent_config::linux::programs::find_in("ffmpeg", &std::env::var_os("PATH")?)?;
    for path in [
        std::env::current_exe().ok()?,
        ffmpeg.clone(),
        base.helper_path.clone(),
    ] {
        hash_file(&mut digest, &path)?;
    }
    // A reboot conservatively invalidates driver/hardware assumptions.
    digest.update(std::fs::read("/proc/sys/kernel/random/boot_id").ok()?);
    digest.update(std::fs::read("/proc/sys/kernel/osrelease").ok()?);
    let version = std::process::Command::new(ffmpeg)
        .arg("-version")
        .output_bounded()
        .ok()?;
    if !version.status.success() {
        return None;
    }
    digest.update(version.stdout);
    digest.update(
        blent_config::FileConfig::load()
            .pipe_capacity_mib
            .to_le_bytes(),
    );
    digest.update(std::fs::read("/proc/sys/fs/pipe-max-size").ok()?);
    Some(format!("{:x}", digest.finalize()))
}
