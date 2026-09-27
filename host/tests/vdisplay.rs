//! T645: discovery must be covered without inspecting or changing host devices.
#![cfg(target_os = "linux")]

use blent::vdisplay::{evdi_cards, evdi_connectors};
use std::{os::unix::ffi::OsStrExt, path::Path, process::Command};

const CHILD: &str = "BLENT_T645_PRIVATE_SYSFS";

fn directory(root: &Path, name: &str) {
    std::fs::create_dir_all(root.join(name)).unwrap();
}

fn populated(root: &Path) {
    for path in [
        "devices/platform/evdi.0/drm/card11",
        "devices/platform/evdi.1/drm/card2",
        "devices/platform/evdi.2/drm/card2",
        "devices/platform/evdi.2/drm/card4294967295",
        "devices/platform/physical/drm/card4",
        "devices/platform/evdi.broken",
        "class/drm/card2-DVI-I-1",
        "class/drm/card11-DVI-I-2",
        "class/drm/card4294967295-HDMI-A-1",
        "class/drm/card4-DVI-I-3",
        "class/drm/card9-DVI-I-4",
    ] {
        directory(root, path);
    }
    std::fs::write(
        root.join("devices/platform/evdi.broken/drm"),
        b"not a directory",
    )
    .unwrap();
    for invalid in ["card", "card-1", "card4294967296", "renderD128", "junk"] {
        directory(root, &format!("devices/platform/evdi.0/drm/{invalid}"));
        directory(root, &format!("class/drm/{invalid}"));
    }
    for invalid in ["bad-DVI-I-1", "cardx-DVI-I-1", "card4294967296-DVI-I-1"] {
        directory(root, &format!("class/drm/{invalid}"));
    }
    for parent in [
        "devices/platform",
        "devices/platform/evdi.0/drm",
        "class/drm",
    ] {
        std::fs::create_dir(
            root.join(parent)
                .join(std::ffi::OsStr::from_bytes(b"bad\xff")),
        )
        .unwrap();
    }
    std::fs::write(root.join("class/drm/card2-DVI-I-1/status"), b"connected\n").unwrap();
    std::fs::write(root.join("class/drm/card2-DVI-I-1/edid"), [0, 255, 1, 128]).unwrap();
    std::fs::write(
        root.join("class/drm/card11-DVI-I-2/status"),
        b"disconnected\n",
    )
    .unwrap();
}

#[test]
fn t645_sysfs_discovery_is_independent_of_host_evdi() {
    if std::env::var_os(CHILD).is_none() {
        let root = tempfile::tempdir().unwrap();
        directory(root.path(), "devices/platform");
        directory(root.path(), "class/drm");
        let output = Command::new("unshare")
            .args([
                "--user",
                "--map-root-user",
                "--mount",
                "--propagation",
                "private",
                "/bin/sh",
                "-c",
                "set -e; mount --bind \"$1\" /sys; shift; exec \"$@\"",
                "blent-t645",
            ])
            .arg(root.path())
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "t645_sysfs_discovery_is_independent_of_host_evdi",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "T645: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = Path::new("/sys");
    assert!(evdi_cards().is_empty());
    assert!(evdi_connectors().is_empty());
    populated(root);
    assert_eq!(evdi_cards(), [2, 11, u32::MAX]);
    let found = evdi_connectors();
    assert_eq!(
        found
            .iter()
            .map(|c| (c.card, c.name.as_str(), c.connected))
            .collect::<Vec<_>>(),
        [
            (2, "DVI-I-1", true),
            (11, "DVI-I-2", false),
            (u32::MAX, "HDMI-A-1", false)
        ]
    );
    assert_eq!(found[0].edid, [0, 255, 1, 128]);
    assert!(found[1..].iter().all(|c| c.edid.is_empty()));
    std::fs::remove_dir_all(root.join("devices/platform")).unwrap();
    assert!(evdi_cards().is_empty());
    assert!(evdi_connectors().is_empty());
    std::fs::remove_dir_all(root.join("class/drm")).unwrap();
    assert!(evdi_connectors().is_empty());
}
