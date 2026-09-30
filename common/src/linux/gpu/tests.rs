use super::*;
use std::os::unix::fs::symlink;

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dri = root.path().join("dri");
    let sys = root.path().join("sys");
    std::fs::create_dir_all(dri.join("by-path")).unwrap();
    std::fs::create_dir_all(sys.join("renderD128/device")).unwrap();
    std::fs::write(dri.join("renderD128"), "").unwrap();
    symlink("../renderD128", dri.join("by-path/pci-first-render")).unwrap();
    std::fs::write(
        sys.join("renderD128/device/uevent"),
        "DRIVER=amdgpu\nPCI_ID=1002:73FF\n",
    )
    .unwrap();
    (root, dri, sys)
}

#[test]
fn t727_discovery_checks_stable_identity_permissions_and_native_bounds() {
    let (root, dri, sys) = fixture();
    let devices = discover_in(&dri, &sys, |_| true);
    assert_eq!(devices.len(), 1);
    assert!(devices[0].adapter.label.starts_with("AMD "));
    assert_eq!(devices[0].adapter.id, "vaapi:pci-first-render");
    assert_eq!(devices[0].node, dri.join("renderD128"));
    assert!(catalog(&devices).supported);
    assert!(!discover_in(&dri, &sys, |_| false)[0].adapter.accessible);
    assert!(discover_in(&root.path().join("missing"), &sys, |_| true).is_empty());
    for name in ["card", "pci-card", "missing-render"] {
        std::fs::write(dri.join("by-path").join(name), "").unwrap();
    }
    symlink(root.path(), dri.join("by-path/outside-render")).unwrap();
    std::fs::write(dri.join("invalid"), "").unwrap();
    symlink("../invalid", dri.join("by-path/invalid-render")).unwrap();
    let long = format!("{}-render", "a".repeat(200));
    symlink("../renderD128", dri.join("by-path").join(long)).unwrap();
    assert_eq!(discover_in(&dri, &sys, |_| true).len(), 1);
    for (id, expected) in [
        ("8086:1234", "Intel"),
        ("10DE:0011", "NVIDIA"),
        ("ABCD:0000", "GPU"),
    ] {
        assert!(device_label("id", &format!("PCI_ID={id}"), "").starts_with(expected));
    }
    assert!(device_label("id", "", "").contains("unknown driver"));
    std::fs::remove_file(sys.join("renderD128/device/uevent")).unwrap();
    assert_eq!(discover_in(&dri, &sys, |_| true).len(), 1);
    let native = discover(); // Actual Linux inventory and read/write permission check.
    assert!(native
        .iter()
        .all(|device| device.node.starts_with("/dev/dri")));
}

#[test]
fn t727_effective_reports_require_owned_process_and_open_device_evidence() {
    let (root, dri, sys) = fixture();
    let devices = discover_in(&dri, &sys, |_| true);
    let proc = root.path().join("proc");
    let process_dir = proc.join("123");
    std::fs::create_dir_all(process_dir.join("fd")).unwrap();
    let mut process = super::super::processes::Process {
        pid: 123,
        uid: 0,
        start_ticks: 1,
        executable: "ffmpeg".into(),
        arguments: vec![
            "ffmpeg".into(),
            "-i".into(),
            "/owned/capture.fifo".into(),
            "-c:v".into(),
            "h264_vaapi".into(),
        ],
        cwd: "/".into(),
    };
    let fifos = vec!["/owned/capture.fifo".into()];
    assert!(observe(&proc, &process, 55, &fifos, &devices).is_none());
    for stat in ["invalid", "123 (ffmpeg) S unknown", "123 (ffmpeg) S 99"] {
        std::fs::write(process_dir.join("stat"), stat).unwrap();
        assert!(observe(&proc, &process, 55, &fifos, &devices).is_none());
    }
    std::fs::write(process_dir.join("stat"), "123 (ffmpeg) S 55").unwrap();
    assert!(observe(&proc, &process, 55, &[], &devices).is_none());
    assert!(observe(&proc, &process, 55, &fifos, &devices)
        .unwrap()
        .contains("unreported"));
    symlink(dri.join("renderD128"), process_dir.join("fd/1")).unwrap();
    assert!(observe(&proc, &process, 55, &fifos, &devices)
        .unwrap()
        .contains("opened GPU: AMD"));
    process.arguments[4] = "libx264".into();
    assert!(observe(&proc, &process, 55, &fifos, &devices)
        .unwrap()
        .contains("CPU / software"));
    process.arguments[4] = "h264_nvenc".into();
    assert!(observe(&proc, &process, 55, &fifos, &devices)
        .unwrap()
        .contains("unreported"));
    process.arguments[4] = "unknown".into();
    assert!(observe(&proc, &process, 55, &fifos, &devices).is_none());
    process.arguments.clear();
    assert!(observe(&proc, &process, 55, &fifos, &devices).is_none());
    assert!(active(0, &fifos, &devices).is_empty());
    assert!(active(std::process::id(), &[], &devices).is_empty());
}

#[test]
fn t727_pci_names_are_optional_bounded_and_do_not_cross_vendor_sections() {
    let data = "1002  AMD\n\t73ff  Radeon RX 6600 XT\n\t\t1002 0001  Variant\n\n8086  Intel\n\t73ff  Other product\n";
    assert_eq!(pci_product(data, "1002:73FF"), Some("Radeon RX 6600 XT"));
    assert_eq!(pci_product(data, "8086:73FF"), Some("Other product"));
    for invalid in ["", "1002", "1002:ffff", "ffff:73ff", ":", "1002:73ff:00"] {
        assert!(pci_product(data, invalid).is_none());
    }
    assert!(
        device_label("pci-first-render", "PCI_ID=1002:73FF\nDRIVER=amdgpu", data)
            .contains("Radeon RX 6600 XT")
    );
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("pci.ids");
    assert_eq!(pci_database(&[&file]), "");
    std::fs::write(&file, data).unwrap();
    assert_eq!(pci_database(&[&root.path().join("absent"), &file]), data);
    std::fs::write(&file, vec![b'x'; 4 * 1024 * 1024 + 1]).unwrap();
    assert_eq!(pci_database(&[&file]).len(), 4 * 1024 * 1024);
}
