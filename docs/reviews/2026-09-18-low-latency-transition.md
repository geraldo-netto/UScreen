# Low-latency profile transition investigation — T429

The Linux encoder selector currently requests a full daemon restart. A reconnect
is therefore expected when switching between `h264_vaapi` and
`h264_vaapi_baseline`. This investigation does **not** establish the cause or
duration of the user's longer searching interval, or prove which profile resumed.
This was the initial isolated investigation. A later
[live encoder-only change](2026-09-18-low-latency-live.md) recorded a missing-FIFO
failure and a roughly 21-second recovery. Subsequent
[T429 regressions and fix](2026-09-18-fifo-ownership.md) address FIFO cleanup by
unowned or retired managers. The user's original option/path remains unconfirmed;
the GUI's deliberate full restart remains distinct from an encoder-only update.

Source inspected: `0458085`, plus the permanent T429 test added with this report.
The checks below use private configuration files, injected restart actions and
isolated tests. They do not restart the installed daemon, attach EVDI, operate the
physical tablet or measure playback latency.

## Established behavior

| Path | Finding | Evidence |
|---|---|---|
| Linux save | An encoder edit displays **Apply & restart**. Saving commits the selected profile before invoking one restart action. | `common/src/model.rs:requires_restart_from`, `gui/src/main.rs:apply`, `gui/src/settings.rs:PendingSave::start_with_pipe` |
| Daemon restart | A managed instance uses a service restart; a direct instance stops before starting. This tears down more than the encoder. | `gui/src/main.rs:restart_with`, `host/src/capture.rs:shutdown` |
| Profile mapping | The explicit low-latency choice maps to stock `h264_vaapi` with `constrained_baseline` and `cavlc`; it retains the selected render node. Async depth one is requested only when advertised. | `common/src/encoding.rs`, `host/src/capture/cli_encoder.rs` T400 tests |
| Persistence and errors | Normal → low latency → normal retains the requested profile and other settings. A failed restart leaves the new saved selection in place and reports the failure; saving alone is not proof of playback. | New `gui/src/settings.rs:t429_profile_switches_persist_before_one_restart_and_report_failure` |
| Automatic selection | The automatic selector requires `encoder == "auto"`; it does not rank or replace an explicit low-latency choice. | `host/src/selection/worker.rs:eligible`, `host/src/media.rs:effective_encoder` |
| Optional encoder build | In-process VAAPI remains unsupported and is rejected before capture. | Existing T284 tests; no FFmpeg patch or new fallback |

The capture supervisor already distinguishes live encoder changes from display
mode changes: it can retain the helper for a live encoder-only update. The Linux
selector currently follows the full restart route above. This distinction is a
possible future way to reduce interruption, not a reproduced cause of the report
or a change made here.

Recovery loops also prevent inferring a cause from the overlay alone.
`ControlSession.scheduleReconnect` delays reconnection after a control failure;
`VideoReceiver` has separate reconnect paths after EOF, timeouts and decoder
retirement. Host setup failures have retry backoff. These paths must be identified
in logs; their configured delays do not measure the reported interruption.

## Automated checks

The following checks passed on the inspected tree:

```sh
cargo test --locked -p uscreen-gui t429
cargo test --locked -p uscreen-gui t400
cargo test --locked -p uscreen --bin uscreen t400
cargo test --locked -p uscreen --bin uscreen t223_setup
cargo test --locked -p uscreen-config encoding::tests
cargo test --locked -p uscreen --features inproc-encoder --bin uscreen encoder::tests
```

The optional build used the existing extracted stock FFmpeg development package
through `PKG_CONFIG_PATH`; no FFmpeg source was changed. T429's new test exercises
current expected behavior and passes without a behavior change. It is diagnostic
coverage, not a claimed red/green fix for the physical report. T223 checks a
settings update during isolated helper setup; it does not prove the real desktop
retains its display through a GUI-triggered daemon restart.

## Evidence still needed

Confirm the exact option originally selected. For a controlled normal → low
latency → normal comparison, record the host/GUI/APK revisions and hashes,
FFmpeg and GPU/driver versions, render node, resolution/FPS, quality and pipe
setting. Keep the other settings constant.

Correlate the Apply event, saved selection, daemon stop/start, encoder command
and stderr, control/video reconnects, decoder setup or failures, and first resumed
render acknowledgement. Verify the actual H.264 profile and record separate
host/tablet clock boundaries rather than subtracting unsynchronised timestamps.
Distinguish one restart from repeated setup or decoder recovery.

This comparison needs an isolated display setup that avoids the unresolved T222
EVDI/Cinnamon attachment failure. Preserve the working settings and establish a
permanent failing regression for any demonstrated cause before a behavioral fix.
The earlier [codec benchmark](../benchmarks/2026-09-18-codecs.md) measures decoder
delay for supplied streams; it does not validate this live transition.
