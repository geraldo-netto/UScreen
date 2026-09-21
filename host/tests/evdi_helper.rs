#![cfg(target_os = "linux")]
//! Hardware-independent regressions for the C capture helper.
//! Run by the normal `cargo test` suite; only a C compiler is required.

use std::{path::PathBuf, process::Command};

struct Harness(PathBuf);

#[test]
fn t492_sparse_capture_lease_preserves_fresh_frame_wakeup() {
    Harness::build("T492").run("T492");
}

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
        } else if matches!(case, "T082" | "T274" | "T340" | "T497" | "T554" | "T492") {
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
fn t554_capture_converts_only_chroma_blocks_intersecting_damage() {
    Harness::build("T554").run("T554");
}

#[test]
fn t474_manual_conversion_capacity_ignores_affinity_and_handles_partial_creation() {
    Harness::build("T474").run("T474");
}

#[test]
fn t343_capture_destination_must_be_a_fifo() {
    Harness::build("T343").run("T343");
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
fn t315_fatal_errors_fail_after_cleanup_and_signals_succeed() {
    let harness = Harness::build("T315");
    for case in ["T315-signal", "T315-channel", "T315-mode"] {
        harness.run(case);
    }
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

#[test]
fn t294_capture_watchdog_recovers_at_poll_deadline() {
    Harness::build("T294").run("T294");
}

#[test]
fn t324_mode_retirement_during_pacing_discards_claimed_frame() {
    Harness::build("T324").run("T324");
}

#[test]
fn t340_unreadable_edid_never_acquires_a_display_device() {
    let harness = Harness::build("T340");
    for case in ["missing", "directory", "empty", "oversized", "readable"] {
        harness.run(&format!("T340-{case}"));
    }
}

#[test]
fn t341_fifo_edid_fails_without_waiting_for_a_writer() {
    Harness::build("T341").run("T341");
}

#[test]
fn t226_partial_fifo_requires_a_replacement_inode() {
    Harness::build("T226").run("T226");
}

#[test]
fn t383_conversion_workers_respect_affinity_and_fallback() {
    Harness::build("T383-affinity").run("T383-affinity");
}

#[test]
fn t405_capture_waits_for_exact_deadline() {
    Harness::build("T405-capture").run("T405-capture");
}

#[test]
fn t405_idle_writer_waits_for_keepalive_deadline() {
    Harness::build("T405-writer").run("T405-writer");
}

#[test]
fn t405_writable_fifo_avoids_redundant_poll() {
    Harness::build("T405-fifo").run("T405-fifo");
}

#[test]
fn t415_requested_pipe_sizes_survive_reopen_and_refusal() {
    Harness::build("T415-open").run("T415-open");
}

#[test]
fn t415_live_resize_keeps_queued_frames_and_retries_busy_or_denied_requests() {
    Harness::build("T415-live").run("T415-live");
}

#[test]
fn t415_report_uses_actual_rounded_kernel_capacity() {
    Harness::build("T415-rounding").run("T415-rounding");
}

#[test]
fn t497_capture_callbacks_startup_and_numeric_fuzz() {
    let harness = Harness::build("T497");
    for case in ["T497-callbacks", "T497-main", "T497-bounds"] {
        harness.run(case);
    }
}

#[test]
fn t418_shared_capture_ownership_pacing_and_control_fuzz() {
    let harness = Harness::build("T418");
    for case in ["T418-ring", "T418-capture", "T418-startup"] {
        harness.run(case);
    }
}
