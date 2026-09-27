//! Shared native ADB executable fixture; no host shell or external ADB server.
pub fn fixture(root: &std::path::Path) {
    let result = std::process::Command::new("rustc")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/usb_adb.rs"))
        .args(["--edition=2021", "-o"])
        .arg(root.join(blent_config::platform::executable_name("adb")))
        .output()
        .unwrap();
    assert!(result.status.success(), "T525: {result:?}");
    std::fs::write(
        root.join("inventory"),
        "List of devices attached\nUSB\tdevice\n",
    )
    .unwrap();
}
