//! T380: public C ownership interfaces, linked without including production C.
use std::{path::PathBuf, process::Command};

#[test]
fn t380_independent_contexts_preserve_frame_and_callback_lifetimes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("modules-test");
    let output = Command::new("cc")
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-pthread",
            "-fsanitize=address,undefined",
            "-fno-pie",
            "-no-pie",
            "-I",
        ])
        .arg(root.join("evdi"))
        .arg(root.join("tests/evdi_modules_test.c"))
        .args(
            [
                "conversion.c",
                "frame_exchange.c",
                "fifo_writer.c",
                "capture.c",
                "writer.c",
            ]
            .map(|name| root.join("evdi").join(name)),
        )
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
