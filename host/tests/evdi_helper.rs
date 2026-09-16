//! Hardware-independent regressions for the C capture helper.
//! Run by the normal `cargo test` suite; only a C compiler is required.

use std::{path::PathBuf, process::Command};

struct Harness(PathBuf);

impl Harness {
    fn build(case: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("uscreen-evdi-test-{}-{case}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let harness = Self(dir);
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/evdi_helper_test.c");
        let output = Command::new("cc")
            .args([
                "-std=c11",
                "-O1",
                "-g",
                "-ffunction-sections",
                "-fdata-sections",
                "-Wl,--gc-sections",
                "-pthread",
            ])
            .arg(source)
            .arg("-o")
            .arg(harness.0.join("helper-test"))
            .output()
            .expect("the EVDI regression suite requires a C compiler (cc)");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        harness
    }

    fn run(&self, case: &str) -> String {
        let output = Command::new(self.0.join("helper-test"))
            .arg(case)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn t012_nv12_crops_odd_dimensions_without_overwriting_buffers() {
    Harness::build("T012").run("T012");
}

#[test]
fn t013_mode_changes_announce_actual_stream_dimensions() {
    let output = Harness::build("T013").run("T013");
    let sizes: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with("STREAM_SIZE "))
        .collect();
    assert_eq!(sizes, ["STREAM_SIZE 6 4", "STREAM_SIZE 10 6"], "{output}");
}

#[test]
fn t047_failed_worker_creation_keeps_conversion_live() {
    Harness::build("T047").run("T047");
}

#[test]
fn t048_rejects_unsupported_framebuffer_formats() {
    Harness::build("T048").run("T048");
}

#[test]
fn t049_poll_timeout_preserves_frame_for_a_live_reader() {
    Harness::build("T049").run("T049");
}

#[test]
fn t050_latency_samples_are_synchronized_with_statistics() {
    Harness::build("T050").run("T050");
}

#[test]
fn t051_picks_lowest_card_including_card_zero() {
    Harness::build("T051").run("T051");
}

#[test]
fn t052_zero_bytes_written_means_device_creation_failed() {
    Harness::build("T052").run("T052");
}

#[test]
fn t081_idle_writer_releases_buffers_before_mode_changes() {
    let output = Harness::build("T081").run("T081");
    assert_eq!(
        output
            .lines()
            .filter(|line| line.starts_with("STREAM_SIZE "))
            .count(),
        4
    );
}
