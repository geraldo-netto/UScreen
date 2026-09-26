//! T533: dependency evidence must not be confused with runtime readiness.
#![cfg(feature = "platform")]
use blent_config::diagnostics::{collect_with, Probe, State, Tool};
use std::path::{Path, PathBuf};

struct Fake {
    present: bool,
    output: Result<Vec<u8>, String>,
    calls: Vec<String>,
}
impl Probe for Fake {
    fn find(&mut self, name: &str) -> Option<PathBuf> {
        self.present
            .then(|| PathBuf::from(format!("tools café/{name}.exe")))
    }
    fn run(&mut self, path: &Path, argument: &str) -> Result<Vec<u8>, String> {
        self.calls.push(format!("{} {argument}", path.display()));
        self.output.clone()
    }
}
fn unsupported() -> blent_config::platform::Capabilities {
    blent_config::platform::Capabilities {
        camera: false,
        daemon: false,
        display: false,
        input: false,
        system_setup: false,
        autostart: false,
        pipe_capacity: false,
        conversion_pool: false,
    }
}
fn fake(output: Result<Vec<u8>, String>) -> Fake {
    Fake {
        present: true,
        output,
        calls: vec![],
    }
}

#[test]
fn t533_missing_commands_never_run_and_backends_stay_unsupported() {
    let mut probe = fake(Ok(vec![]));
    probe.present = false;
    let report = collect_with(&mut probe, unsupported());
    assert!(probe.calls.is_empty());
    assert!(report.tools.iter().all(|tool| tool.state == State::Missing));
    assert!(report.tools.iter().all(|tool| tool.path.is_none()));
    let text = report.lines().join("\n");
    for expected in [
        "ADB: missing",
        "FFmpeg: missing",
        "Display: unavailable (unsupported)",
        "Input: unavailable (unsupported)",
        "Tablet connection: unverified",
    ] {
        assert!(text.contains(expected), "T533: {text}");
    }
}
#[test]
fn t533_versions_paths_and_command_failures_are_separate_evidence() {
    let mut probe = fake(Ok(
        b"ffmpeg version 6.1.6 Copyright\nconfiguration: fixture\n".to_vec(),
    ));
    let report = collect_with(&mut probe, unsupported());
    assert!(matches!(report.tools[0].state, State::Unverified(_)));
    assert_eq!(report.tools[1].state, State::Available("6.1.6".into()));
    assert!(report.tools[1].verified());
    assert!(!report.tools[0].verified());
    assert_eq!(report.tools[1].tool, Tool::Ffmpeg);
    assert!(report.lines().join("\n").contains("tools café/ffmpeg.exe"));
    assert!(probe.calls[0].ends_with("adb.exe version"));
    assert!(probe.calls[1].ends_with("ffmpeg.exe -version"));
    let mut probe = fake(Ok(
        b"Android Debug Bridge version 1.0.41\nVersion 36.0.0-13206524\nInstalled as fixture\n"
            .to_vec(),
    ));
    let report = collect_with(&mut probe, unsupported());
    assert_eq!(
        report.tools[0].state,
        State::Available("36.0.0-13206524 (protocol 1.0.41)".into())
    );
    for error in [
        "command exited with status 7",
        "command timed out",
        "permission denied",
    ] {
        let report = collect_with(&mut fake(Err(error.into())), unsupported());
        assert!(report
            .tools
            .iter()
            .all(|tool| tool.state == State::Unverified(error.into())));
    }
}
#[test]
fn t533_malformed_bounded_output_never_establishes_readiness() {
    for bytes in [
        vec![],
        vec![0xff],
        b"ffmpeg version\n".to_vec(),
        b"ffmpeg version nope\n".to_vec(),
        b"ffmpeg version 6.1\x1b[0m\n".to_vec(),
        vec![b'x'; 32769],
    ] {
        let report = collect_with(&mut fake(Ok(bytes)), unsupported());
        assert!(report
            .tools
            .iter()
            .all(|tool| matches!(tool.state, State::Unverified(_))));
    }
    for value in 0..=255u8 {
        let mut bytes = b"ffmpeg version ".to_vec();
        bytes.push(value);
        let report = collect_with(&mut fake(Ok(bytes)), unsupported());
        assert!(matches!(report.tools[0].state, State::Unverified(_)));
        assert!(report.lines().iter().all(|line| !line.contains('\x1b')));
    }
    let mut caps = unsupported();
    caps.display = true;
    let report = collect_with(
        &mut fake(Ok(b"Android Debug Bridge version 1.0.41\n".to_vec())),
        caps,
    );
    assert_eq!(report.tools[0].state, State::Available("1.0.41".into()));
    assert!(report
        .lines()
        .join("\n")
        .contains("Display: unverified (backend implemented; runtime not checked)"));
}
