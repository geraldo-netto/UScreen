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
