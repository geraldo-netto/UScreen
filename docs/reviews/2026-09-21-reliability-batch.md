# Four-item implementation batch

The maintainer requested one commit per numbered item, grouping T551/T559 in
item 1 and T558/T549 in item 3. Other findings are recorded separately in
`TODO.md`, without expanding this batch to fix them.

## Item 1 — GUI registration and configuration diagnostics

T551: AppImage registration now creates a GUI compatibility launcher that
selects the same registered image or extracted AppDir as the desktop entry.
The previous `~/.local/bin/uscreen-gui` file/link is moved into a unique
`gui-backup.*` directory inside the installation. Repeated registration is
idempotent; failed link creation restores the previous launcher. Symlink
targets remain untouched. The launcher forwards literal arguments without
embedding user paths in shell source.

T559: configuration logging compares parsed TOML values using complete paths.
An unchanged save produces no change message; changing camera bitrate reports
only `camera.bitrate`, without false display width/height/FPS changes. Quoted
keys stay distinct from nested paths. This affects diagnostics only.

Permanent tests first reproduced the stale GUI and conflicting display/camera
logs, then passed after the fixes. Retained coverage includes image/AppDir
upgrades, spaces and shell metacharacters, symlink targets, backup/idempotence,
failed-link rollback, semantic TOML equivalence, additions/removals, quoted
keys, malformed input and bounded nested paths.

Validation: 28 AppImage tests, two configuration-log regressions, two semantic
diff tests and the normal common/library suites passed. Essential-script
coverage passed for all 61 Python and 60 shell functions. Scoped Rust coverage
measured all five new diff functions at 100% and changed `write_at` at 80.95%;
this scoped run is not a replacement for the existing full-platform report.
Complexity checked 5,157 functions with none above 9.

Raw red/green logs and coverage reports are retained locally under
`/tmp/uscreen-item1-*`. New findings T566 (log emitted before persistence),
T567 (pre-existing forwarding-module formatting), and T568 (pending-frame
accounting) remain open for later review. No live daemon, display, tablet,
kernel module or installed package was restarted by this item.

## Item 2 — adaptive idle capture

T492 adds an experimental, default-off `adaptive_idle` preference in the host
Video tab. It currently supports the Linux CLI encoder and automatic selection
with a named Android decoder. Explicit encoder choices without a named decoder
and the optional in-process encoder retain five idle updates/s. Fresh damage
still wakes the writer immediately and remains subject to the user's motion FPS.

Portable policy in `common/src/idle.rs` measures 16 compatible-cadence samples,
then tries 16 sparse-cadence samples for the current encoder epoch. Admission
requires consecutive frame identities, monotonic unreordered media timestamps,
matching named-decoder receipts, ACK progress and periodic keyframes. Packet-ready
to ACK must stay within 100 ms; media-timestamp-to-ACK timing must remain within
20 ms of its baseline. The latter is a *relative change* measurement: it detects
encoder/output queue growth without claiming an absolute raw-write-to-display
latency or an optical presentation measurement. Missing evidence, clock jumps,
slow ACKs, keyframe gaps or a 1.5-second stall restore five updates/s. Reconnect,
encoder, decoder and stream changes require new evidence. No admission state is
persisted. Every admitted new video viewer invalidates the running certificate,
even across a reconnect with no skipped sequence; that session retains five
updates/s until a new encoder epoch. `Selected.verified` is not used as a sparse-latency certificate.

The Linux adapter publishes a private bounded record tied to the FIFO's device
and inode, a live session token, and a 1.5-second monotonic lease. The helper
rejects expired, malformed, overlong, overflowing, mismatched, symlinked or
writable-by-others requests. Controller loss expires back to compatibility;
normal cancellation removes only its own record and prevents further writes.
The stock FFmpeg wall-time keyframe expression moves from 1.0 to 0.9 seconds
when this option is enabled, allowing timestamp rounding before a sparse tick.
No FFmpeg source change or additional Android thread is required.

Permanent configuration and native-writer regressions were first observed red,
then green. Retained cases cover fresh damage around idle deadlines, lease
expiry, bounded invalid inputs, unchanged motion FPS, baseline/trial/fallback,
timestamp conversion/reordering, slow clients, missing keys/receipts, history
bounds and session retirement. Existing final-packet, late-join, timestamp and
watchdog regressions remain in the normal suite.

This option was not enabled on the active session. Native end-to-end sparse
admission and sustained power measurements remain pending under T492; isolated
writer counts and synthetic timing tests are not measured full-pipeline gains.

Item-2 validation: 86 common, 19 host-library, 480 host-binary and 66 GUI unit
tests passed, plus six isolated daemon/Wi-Fi/GUI integration tests. The optional
in-process build and its 366-test suite passed; it retains the original idle
cadence. Nine existing T448 framing/late-join tests also passed against bundled
FFmpeg 6.1.6. Three existing default-build performance experiments remain ignored
by their pre-existing attributes; none was removed or disabled by this change.
All 119 C capture functions and all 68 functions in the explicit policy,
controller, framing and video-queue coverage scope meet 80%. New/changed GUI and
configuration/process wiring are included in the separate 188-function wiring
report, which also passes the 80% gate after combining CLI and in-process
counters. The final in-process capture rerun passed all 27 tests.
Complexity checked 5,229 functions with none above 9. Local evidence is retained
under `/tmp/uscreen-item2-*`; the source snapshots and reports identify their
scopes and do not claim Windows/macOS native validation.

## Item 3 — stock EVDI behavior and Android lock evidence

The maintainer chose to keep stock libevdi. Its roughly twelve-second startup
wait is now [documented at the source/API boundary](../benchmarks/2026-09-20-evdi-startup/README.md):
the nominal five-second limit counts sleeps, while repeated process scans add
CPU and wall time. Root Xorg process metadata is unreadable to the ordinary
user. The stock public API cannot bound that private scan, and its subsequent
master/slave checks must remain intact. T558 moves to deferred under that
explicit decision; no local library patch was made.

[Read-only Android evidence](../benchmarks/2026-09-21-android-lock/README.md)
shows the current screen and CPU wake locks active and the last recorded sleep
reason `force_suspend`. Available logs cannot identify its caller or link it
to the historical report. T549 remains blocked on that missing event/action
correlation. No unsupported lock workaround or behavioral fix is claimed.
Documentation links and source paths were checked; this item changes no
production behavior and needs no artificial regression test.

## Item 4 — omitted-frame correlation and missing diagnostics

The [six-event analysis](../benchmarks/2026-09-21-frame-omissions/README.md)
shows every unlatched buffer followed by a newer Queue event before that
successor's latch. Nearby input and output events both arrive in bursts; the
trace cannot identify whether capture, encoding, transport or receiver
scheduling introduced them. Callback ACKs and physical presentation remain
separate measurements. No new Android thread or frame-order change was made.

The diagnostic gap is fixed: opt-in `uscreen::frame_timing=trace` events expose
packet-ready epoch/sequence/media timing and accepted render ACKs. Default
logging stays at INFO. The permanent T561 test first failed with zero packet
records, then passed with two correctly identified packets and one accepted
ACK, rejecting wrong-decoder and duplicate observations. All 481 default host
unit tests passed; three pre-existing performance experiments remain ignored.
All 46 functions in the changed CLI-encoder/latency coverage scope meet 80%;
the nine optional in-process latency regressions also pass. All six correlations
were independently checked against T560’s committed frame-event CSV.
Complexity checked 5,231 functions with none above 9. Local red/green logs,
coverage snapshot and report remain under `/tmp/uscreen-item4-*`.

T561 remains unresolved on a correlated native capture/encode/transport trace
from a safe future normal startup; the historical host aggregates cannot be
turned into missing per-frame evidence. This commit fixes observability and
records the investigation, not an unproven frame-delivery defect.
