#![cfg(target_os = "linux")]
//! T383: bit-exact conversion and dirty-history contracts, without EVDI.
use std::{path::PathBuf, process::Command};

fn run(case: &str) {
    run_with_flags(case, &["-fsanitize=address,undefined"]);
}

fn run_with_flags(case: &str, extra: &[&str]) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("conversion-test");
    let compile = Command::new("cc")
        .args([
            "-std=c11", "-O3", "-g", "-pthread", "-Wall", "-Wextra", "-Werror", "-fno-pie",
            "-no-pie",
        ])
        .args(extra)
        .arg("-I")
        .arg(root.join("evdi"))
        .arg(root.join("tests/conversion_test.c"))
        .arg(root.join("evdi/conversion.c"))
        .arg(root.join("evdi/frame_exchange.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let output = Command::new(binary).arg(case).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t383_conversion_matches_independent_oracle_and_preserves_three_histories() {
    run("equivalence");
}

#[test]
fn t383_empty_and_tiny_damage_does_not_wake_the_pool() {
    run("dispatch");
}

#[test]
fn t452_extreme_damage_endpoints_clip_without_overflow() {
    run("damage-extreme");
}

#[test]
fn t454_empty_damage_intervals_preserve_all_histories() {
    run("damage-empty");
}

#[test]
fn t383_large_pool_supports_128_workers_and_joins_them() {
    run("large");
}

#[test]
fn t383_large_pool_balances_clustered_damage_without_waking_idle_workers() {
    run("density");
}

#[test]
fn t383_release_scalar_and_auto_vectorized_kernels_match_the_oracle() {
    run_with_flags("equivalence", &[]);
    run_with_flags(
        "equivalence",
        &["-fno-tree-vectorize", "-fno-tree-slp-vectorize"],
    );
}

#[test]
fn t554_regions_clip_fuzzed_bounds_preserve_pixels_and_follow_buffer_leases() {
    run("regions");
    run_with_flags("regions", &[]);
}
