//! The normal Linux suite retains the platform-free target contract (T736).
#![cfg(all(target_os = "linux", feature = "platform"))]

#[test]
fn t736_policy_builds_without_native_entropy_on_webassembly() {
    let target = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "check",
            "--locked",
            "-p",
            "blent-config",
            "--no-default-features",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .env("CARGO_TARGET_DIR", target.path())
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "T736: install wasm32-unknown-unknown before running the suite; policy must compile without native adapters:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
