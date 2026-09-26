# Blent follow-up validation

## T588: deterministic VP9 timing markers

Pre-rename revision `90b80ee98a6f474b824c4f27ffa4c6ac92a6d3c9` reproduces
T421's first-pixel failure (7 instead of 16) with Debian FFmpeg 5.1.9,
libvpx 1.12.0 and libaom 3.6.0. Direct raw fixture output has exactly uniform
luma 16 and chroma 128. Explicit limited-range BT.709 tags do not change the
failure. Decoded lossy VP9's first frame contains luma values from 1 through 39;
AV1 preserves the expected markers. This is a fixture's invalid assumption about
lossy pixel precision, not a rename regression or demonstrated range mismatch.

The fixture now overrides VP9 quality with `-lossless 1 -crf 0`. All production
timing options and the original frame count, picture order, monotonic timestamps,
keyframe schedule and pixel-tolerance assertions remain. Application encoder
settings are unchanged. Permanent T588 additionally checks every luma/chroma
sample in every decoded VP9 frame, exactly; it failed before the fixture fix.

All three timestamp tests pass with both Debian FFmpeg 5.1.9 and bundled FFmpeg
6.1.6. Logs: [red](artifacts/2026-09-26-blent-validation/t588-red.log.gz),
[pre-rename](artifacts/2026-09-26-blent-validation/t588-baseline.log.gz),
[FFmpeg 5](artifacts/2026-09-26-blent-validation/t588-green-ffmpeg5.log.gz),
[FFmpeg 6](artifacts/2026-09-26-blent-validation/t588-green-ffmpeg6.log.gz).

Reproduce in either supported validation environment:

```sh
cargo test --locked --release -p blent --bin blent capture::cli_encoder::timestamp_tests -- --nocapture
```

## T573: strict workspace lint

Normalized two test-only parity checks to `is_multiple_of(2)`. Clippy on the
repository's Rust 1.90.0 validation toolchain reproduced `manual_is_multiple_of`
before the change. `cargo clippy --locked --workspace --all-targets --all-features
-- -D warnings` passes afterward. No production behavior changed.

## T590: remaining command, scheduling and host coverage

Eight gaps are closed by retained isolated tests and native Linux evidence:

| Function | Executable-line coverage |
| --- | ---: |
| `commands::terminate_group` | 85.71% |
| `scheduling::linux::apply_pid` | 100% |
| `scheduling::linux::apply_server` | 100% |
| `scheduling::apply_configured` | 100% |
| `CliEncoder::log_encoder_dimensions` | 100% |
| `latency::Report::log` | 100% |
| `ensure_single_daemon` | 88.24% |
| `spawn_extra_session` | 100% |

T590 adds a child-local seccomp denial of `kill`, verifies the warning without
signalling real processes, captures pre-subscriber diagnostics in isolated
processes, checks idle session listener retirement without opening devices, and
extends the private-namespace daemon test to refuse an untracked live owner.
The existing T582 native test runs on this Linux desktop, verifying effective
weights, thread/child inheritance, a private fake ADB process and denied manager
fallback. It never changes the real ADB server's scheduling.

Default workspace: 810 pass, three pre-existing ignored tests. Two tooling
tests initially lacked system Python modules in the isolated venv; their reruns
passed after exposing the installed system packages. Other passing counters
were retained, and the remaining suites completed. Optional in-process host
library/binary: 401 pass, two pre-existing ignored tests. Strict all-target,
all-feature workspace Clippy and formatting pass. Complexity: 5,622 functions,
none above nine.

Final combined Linux scope: 1,220/1,222 functions meet 80%, none unmeasured.
**The overall gate still fails** for T572's camera `open_device` (75%) and
`run_native` (28.57%). All 61 non-Linux functions remain unmeasured in the full
report; no foreign-platform runtime validation is claimed.

The [artifact directory](artifacts/2026-09-26-blent-validation/) retains the exact
source manifest, merged default/optional LLVM counters, additional native
scheduling counters, reports and logs. Native counters use the unchanged common
production sources; test-only additions do not alter those source locations.

## T577: X11 input follows connector identity

Native acceptance reproduced the recorded mismatch: DRM card1's `DVI-I-1`
appeared as RandR `DVI-I-2-1`; the previous name-prefix logic left touch addressing
the whole desktop. A permanent regression failed before the selection fix.

Linux discovery now supplies the owned connector's EDID. The X11 adapter queries
RandR properties, validates bounded EDID headers, extension counts and block
checksums, and selects exactly one active output with exactly one eligible owner.
It rejects absent, corrupt and duplicated identities rather than guessing by
provider-dependent names. Physical-screen selection also excludes known virtual
identities. Other OS backends and wire contracts are unchanged.

Tests retain T029's per-tablet/device assertions and add renamed two-card cases,
ambiguous/missing identities, physical-mode selection, invalid-length and
checksum mutation loops. Input suite: 68 pass. Clean host library/binary
collection: 523 pass, three pre-existing ignored tests. Every one of the 23
mapping/discovery functions meets 80%; all six new identity functions reach
100%. Strict all-target/all-feature Clippy and formatting pass. Whole-project
complexity: 5,631 functions, none above nine. Camera coverage remains T572.

The rebuilt AppImage was installed and the service restarted. Touch and pointer
read back the expected matrix for the 1280x800 output at desktop x=3840:
`[0.25, 0, 0.75; 0, 0.370370, 0; 0, 0, 1]`. An Android center tap `(640,400)`
through the real app/USB/uinput path placed the desktop pointer at `(4479,399)`
inside the owned test window. The prior pointer position was restored.

Xorg's base pen node remains a keyboard-like node and rejects `map-to-output`
with `BadMatch`; this can precede creation of separate stylus tool devices.
T592 records late-tool investigation and physical stylus validation. No pen
pressure/tilt success is inferred from the working touch path.

Red/green logs, exact snapshot and clean counters are retained as `t577-*` in
[validation artifacts](artifacts/2026-09-26-blent-validation/).
