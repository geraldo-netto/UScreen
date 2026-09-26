# T613: temporary single-worker libx264 default

At the maintainer's request, the shared encoding profile now supplies
`threads=1` for libx264. Both stock FFmpeg CLI output encoding and the optional
in-process libavcodec adapter consume this policy. Other codecs, automatic
codec selection, conversion workers and runtime workers retain their existing
policies. No new dependency or configuration surface is introduced.

This selects the worker setting behind T600's approximately 1,950 completed
FFmpeg futex calls per five-second scene trace. It does not guarantee that call
rate for other workloads, resolutions or codec selections. T600's measured
CPU/latency gains apply to its 1280×800 fixture; T612 remains the paused follow-up
for bounded encoder-first tuning and user overrides.

The permanent `t613_x264_defaults_to_one_worker_in_both_adapters` test failed
before the change and passes afterward. It checks the CLI and in-process
policy and that other codecs do not inherit the worker limit. The existing
in-process dictionary regression now expects the explicitly changed default.
Four encoding-policy tests and nine native in-process encoder tests pass.
A fresh LLVM coverage collection measures all ten production functions in
`common/src/encoding.rs` at or above 80%, including `codec_options` at 100%.
The complexity gate reports 5,740 functions, none above nine; formatting passes.

Build, packaging and tests used cached local dependencies with Cargo offline.
The AppImage reuses the already verified Debian 12 application tree and unchanged
bundled libraries/runtime; its corresponding dependency sources remain in the
T610 archive. The updated host binary passed ABI/RPATH/load checks. The rebuilt
AppImage was installed and the normal service restarted. After automatic codec
trials settled, the live encoder command contained `-c:v libx264 -threads 1`
and the process had four total threads, matching the earlier single-worker
fixture. The first verification observation fell between calibration processes;
the post-calibration readback, not that transient absence, is acceptance evidence.
The pointer is visible and the user's configuration checksum is unchanged.

[Curated evidence](artifacts/2026-09-26-followup/t613/) retains red/green results,
native encoder tests, function coverage, complexity and deployment readback.
Complete local build/coverage logs, source snapshots, previous/new AppImages
and hashes are under `~/.local/share/blent/profiles/2026-09-26-followup/t613/`.
The rest of the queue is stopped as requested; no camera was enabled.
