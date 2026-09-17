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
        let mut compiler = Command::new("cc");
        if case == "T083" {
            compiler.args(["-fsanitize=thread", "-fno-pie", "-no-pie"]);
        } else if case == "T254" {
            compiler.args(["-fsanitize=undefined", "-fno-sanitize-recover=undefined"]);
        } else if matches!(case, "T082" | "T274") {
            compiler.args(["-fsanitize=address,undefined", "-fno-pie", "-no-pie"]);
        }
        let output = compiler
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
            "compile helper regression {case}: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        harness
    }

    fn run(&self, case: &str) -> String {
        let binary = self.0.join("helper-test");
        let output = if case == "T083" {
            // New kernels can place libraries inside GCC TSan's fixed shadow
            // range. Disable ASLR for this child only when permitted.
            let isolated = Command::new("setarch")
                .arg(std::env::consts::ARCH)
                .arg("-R")
                .arg(&binary)
                .arg(case)
                .env("TSAN_OPTIONS", "halt_on_error=1")
                .output();
            match isolated {
                Ok(output)
                    if !String::from_utf8_lossy(&output.stderr)
                        .contains("Operation not permitted") =>
                {
                    output
                }
                _ => Command::new(&binary)
                    .arg(case)
                    .env("TSAN_OPTIONS", "halt_on_error=1")
                    .output()
                    .unwrap(),
            }
        } else {
            Command::new(&binary).arg(case).output().unwrap()
        };
        assert!(
            output.status.success(),
            "helper regression {case}: {}\n{}",
            output.status,
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

#[test]
fn t083_mode_changes_and_signal_shutdown_are_race_free() {
    Harness::build("T083").run("T083");
}

#[test]
fn t113_stalled_fifo_obeys_deadline_mode_change_and_shutdown() {
    Harness::build("T113").run("T113");
}

#[test]
fn t082_small_modes_never_exceed_scaled_source_bounds() {
    Harness::build("T082").run("T082");
}

#[test]
fn t108_helpers_reserve_distinct_cards_and_find_new_devices() {
    Harness::build("T108").run("T108");
}

#[test]
fn t170_helper_options_preserve_bounds_and_missing_values() {
    Harness::build("T170").run("T170");
}

#[test]
fn t254_conversion_epochs_wrap_without_overflow_or_stalled_workers() {
    Harness::build("T254").run("T254");
}

#[test]
fn t272_failed_event_channel_exits_without_spinning() {
    Harness::build("T272").run("T272");
}

#[test]
fn t274_delayed_writer_cannot_read_replaced_mode_buffers() {
    Harness::build("T274").run("T274");
}

#[test]
fn t279_failed_mode_allocations_retire_capture() {
    Harness::build("T279").run("T279");
}

#[test]
fn t290_capture_poll_deadlines_survive_long_uptimes() {
    Harness::build("T290").run("T290");
}

#[test]
fn t293_nv12_preserves_neutral_gray_and_bt709_colors() {
    Harness::build("T293").run("T293");
}
