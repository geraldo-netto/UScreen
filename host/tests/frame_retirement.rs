//! T389: observable wakeup/deadline contract and retained native frame storage.
use std::{path::PathBuf, process::Command};

#[test]
fn t389_release_notifies_retirement_without_periodic_wakes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("retirement");
    let output = Command::new("cc")
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-pthread",
            "-fsanitize=address,undefined",
            "-fno-pie",
            "-no-pie",
        ])
        .arg(root.join("tests/frame_retirement_test.c"))
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
