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
                .with_file_name("tuned-profiles")
                .join(format!("{fingerprint}.json"));
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
            base.gpu_policy,
            base.ten_bit,
            base.stream_scale,
            base.conversion_threads,
            base.encoder_workers,
            base.calibration_generation,
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
    // T714: reboot alone is not an environment change.
    digest.update(platform_stamp(
        Path::new("/proc"),
        Path::new("/sys"),
        &base.vaapi_device,
    )?);
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
    Some(format!("{:x}", digest.finalize()))
}

// Linux adapter: identify the render device and bound driver changes without boot IDs.
fn device_stamp(sys: &Path, node: &str) -> Vec<u8> {
    let resolved = std::fs::canonicalize(node).unwrap_or_else(|_| node.into());
    let Some(name) = resolved.file_name() else {
        return Vec::new();
    };
    let device = sys.join("class/drm").join(name).join("device");
    ["uevent", "revision", "driver/module/version"]
        .into_iter()
        .flat_map(|field| std::fs::read(device.join(field)).unwrap_or_default())
        .collect()
}

fn platform_stamp(proc: &Path, sys: &Path, node: &str) -> Option<Vec<u8>> {
    let mut stamp = std::fs::read(proc.join("sys/kernel/osrelease")).ok()?;
    stamp.extend(std::fs::read(proc.join("sys/fs/pipe-max-size")).ok()?);
    stamp.extend(device_stamp(sys, node));
    Some(stamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t743_stable_render_alias_retains_device_and_driver_identity() {
        let root = tempfile::tempdir().unwrap();
        let node = root.path().join("renderD128");
        let alias = root.path().join("pci-0000:03:00.0-render");
        std::fs::write(&node, "").unwrap();
        std::os::unix::fs::symlink(&node, &alias).unwrap();
        let device = root.path().join("class/drm/renderD128/device");
        std::fs::create_dir_all(device.join("driver/module")).unwrap();
        std::fs::write(device.join("uevent"), "PCI_ID=1002:73FF").unwrap();
        std::fs::write(device.join("driver/module/version"), "one").unwrap();
        let before = device_stamp(root.path(), node.to_str().unwrap());
        assert!(!before.is_empty());
        assert_eq!(device_stamp(root.path(), alias.to_str().unwrap()), before);
        std::fs::write(device.join("driver/module/version"), "two").unwrap();
        assert_ne!(device_stamp(root.path(), alias.to_str().unwrap()), before);
    }

    #[test]
    fn t714_reboot_preserves_context_but_kernel_driver_and_gpu_changes_do_not() {
        let root = tempfile::tempdir().unwrap();
        let proc = root.path().join("proc");
        let sys = root.path().join("sys");
        assert!(platform_stamp(&proc, &sys, "/dev/dri/renderD128").is_none());
        for (path, value) in [
            ("proc/sys/kernel/osrelease", "6.1"),
            ("proc/sys/kernel/random/boot_id", "boot one"),
            ("proc/sys/fs/pipe-max-size", "1048576"),
            ("sys/class/drm/renderD128/device/uevent", "DRIVER=amdgpu"),
            ("sys/class/drm/renderD128/device/revision", "c1"),
            ("sys/class/drm/renderD128/device/driver/module/version", "1"),
        ] {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, value).unwrap();
        }
        let before = platform_stamp(&proc, &sys, "/dev/dri/renderD128").unwrap();
        std::fs::write(proc.join("sys/kernel/random/boot_id"), "boot two").unwrap();
        assert_eq!(
            platform_stamp(&proc, &sys, "/dev/dri/renderD128").unwrap(),
            before
        );
        for file in [
            "proc/sys/kernel/osrelease",
            "sys/class/drm/renderD128/device/uevent",
            "sys/class/drm/renderD128/device/driver/module/version",
        ] {
            let path = root.path().join(file);
            let saved = std::fs::read(&path).unwrap();
            std::fs::write(&path, "changed").unwrap();
            assert_ne!(
                platform_stamp(&proc, &sys, "/dev/dri/renderD128").unwrap(),
                before
            );
            std::fs::write(path, saved).unwrap();
        }
        assert!(device_stamp(&sys, "/").is_empty());
        assert!(device_stamp(&sys, "/dev/dri/missing").is_empty());
    }
}
