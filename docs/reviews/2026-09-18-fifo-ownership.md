# T429 — preserve the live FIFO during encoder changes

An unstarted `CaptureManager` used to delete the configured slot's FIFO when
dropped or shut down. `capture::tests::t432_capability_metadata_restarts_only_when_effective_stream_changes`
constructed such a manager with default slot 0, so running that metadata test
could remove the installed daemon's capture path. Open pipe descriptors kept
the existing stream alive until an encoder change needed to open the path again.

The [live transition](2026-09-18-low-latency-live.md) recorded precisely that
missing-path failure, followed by helper reattachment and a roughly 21-second
reconnect. Its journal did not trace the unlinking process, so identifying the
specific actor in that historical run remains an inference. The new isolated
regression proves that the cleanup bug deletes a live capture FIFO and prevents
an encoder-only update from recovering. Production C partial-write retirement
does not unlink the path; the initial retirement-ordering hypothesis was not
the demonstrated cause.

## Fix

`capture::fifo::Owned` binds cleanup to the FIFO created by the helper owner.
It retains a Linux `O_PATH` descriptor and device/inode identity. The descriptor
prevents inode reuse without opening either pipe endpoint, changing reader
admission, or masking EOF. Cleanup checks that the owned inode still occupies
the path before removing it. Missing/replaced paths are left alone.

The helper acquires this owner when it creates the FIFO. Partial-frame recovery
atomically replaces the retired FIFO and transfers ownership to the replacement.
Explicit shutdown releases ownership once, after child retirement; subsequent
manager destruction does not remove a later owner's FIFO. Constructing a manager
alone acquires no FIFO ownership and performs no filesystem cleanup. The T432
metadata test also uses the existing distinct-slot fixture rather than slot 0.

This adds one path-only descriptor per active capture FIFO. It does not change
FFmpeg, the wire protocol, codec profiles, Android, saved settings, or the GUI's
deliberate full-daemon restart behavior when applying encoder edits.

## Permanent verification

Four regressions were added and observed failing before changing implementation:

| Regression | Failure before fix / assertion after fix |
| --- | --- |
| `t429_unstarted_managers_preserve_live_fifo` | Unstarted Drop/shutdown removes another capture's FIFO / preserves its inode |
| `t429_retired_owner_preserves_replacement_paths` | Shutdown removes a replacement / preserves a new FIFO, regular file or symlink |
| `t429_shutdown_releases_fifo_ownership_once` | Drop after shutdown removes a new owner's FIFO / cleanup occurs once |
| `t429_encoder_changes_survive_unstarted_manager_cleanup` | No recovery frame after observer Drop / two encoder setting changes yield complete decodable frames, increasing sequence IDs and retired old generations, with one helper start |

A fifth test, `t429_ownership_does_not_hold_fifo_endpoints_open`, verifies that
the ownership descriptor neither admits a writer without a reader nor hides EOF
from a reader without a writer, and that dropping the real owner cleans up.

The streaming regression uses production C FIFO writes, fake DRM, private runtime
paths and software H.264 encode/decode. It runs with both external stock FFmpeg
and the optional in-process libavcodec encoder. It changes encoder quality and
back to exercise the common encoder-replacement path without requiring VAAPI
hardware. Existing T400 tests retain Constrained Baseline argument coverage;
existing T226 tests retain active partial-frame and stale-reset coverage.

Run the new cases with:

```sh
cargo test -p uscreen --bin uscreen t429
cargo test -p uscreen --bin uscreen --features inproc-encoder t429
```

Optional encoding requires the stock FFmpeg development dependencies. All new
capture fixtures use temporary runtime directories and fake helper/display
resources. No installed daemon restart, tablet deployment or physical EVDI
reattachment was performed for this fix. The previously measured live interruption
is historical evidence, not a post-fix timing benchmark.

Recorded results in the [artifact directory](2026-09-18-fifo-ownership/)
(line endings and trailing whitespace normalized):

- [Ownership regressions before the fix](2026-09-18-fifo-ownership/ownership-red.txt): three failures.
- [Streaming regression before the fix](2026-09-18-fifo-ownership/stream-red.txt): recovery-frame timeout.
- [Workspace unit/binary suite](2026-09-18-fifo-ownership/workspace-green.txt): 471 passed, three existing ignored tests; all five T429 tests passed. The suite used a private `XDG_RUNTIME_DIR`.
- [Optional in-process capture suite](2026-09-18-fifo-ownership/inproc-capture-green.txt): 19 passed, including all five T429 tests.
- [Default Clippy](2026-09-18-fifo-ownership/clippy.txt) and [in-process Clippy](2026-09-18-fifo-ownership/inproc-clippy.txt): warnings denied, passed.
- [Complexity](2026-09-18-fifo-ownership/complexity.txt): 3,791 functions, none above nine.
