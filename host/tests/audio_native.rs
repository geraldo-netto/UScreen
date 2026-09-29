//! T718: execute the real adapter against an isolated PipeWire daemon.
#[cfg(all(target_os = "linux", feature = "native-audio"))]
#[test]
fn t718_native_source_pcm_underflow_and_retirement() {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/audio_native.py"
        ))
        .arg(env!("CARGO_BIN_EXE_blent-audio"))
        .arg(env!("CARGO_BIN_EXE_blent"))
        .output()
        .expect("python3 and PipeWire test tools are required");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(all(target_os = "linux", feature = "native-audio"))]
#[test]
fn t719_native_sink_preserves_stereo_and_retires() {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/audio_speakers.py"
        ))
        .arg(env!("CARGO_BIN_EXE_blent-audio"))
        .arg(env!("CARGO_BIN_EXE_blent"))
        .output()
        .expect("python3 and PipeWire test tools are required");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
