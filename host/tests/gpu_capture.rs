#![cfg(target_os = "linux")]
//! T575 adapter policy tests need no GPU, X11 session, or FFmpeg SDK.
#[test]
fn t575_native_bounds_and_connector_identity_survive_invalid_input() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("gpu-options");
    let build = std::process::Command::new("cc")
        .args([
            "-std=c11",
            "-D_POSIX_C_SOURCE=200809L",
            "-O1",
            "-g",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-fsanitize=address,undefined",
            "-fno-pie",
            "-no-pie",
        ])
        .arg(root.join("gpu/options.c"))
        .arg(root.join("gpu/cadence.c"))
        .arg(root.join("tests/gpu_options.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
