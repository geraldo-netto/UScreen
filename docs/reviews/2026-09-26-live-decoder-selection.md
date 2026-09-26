# T627: live decoder selection and capture readiness

The AVC surface-start path constructed `DecoderFormat` without its negotiated
selection. Android decoded and rendered, but its receipt remained absent; the
host correctly rejected ACKs for a named-decoder trial. The internal setup helper
used by earlier tests already preserved selection, which concealed the live-path
bug. A diagnostic native run recorded 620 receipt mismatches while Android logged
rendered output. The installed APK's DEX matched the earlier release build;
this was not an obsolete installation or evidence of decoder failure.

`VideoReceiver.ensureSurfaceCodec` now forwards the selected decoder. A permanent
T627 regression uses that actual entry point, checks the configured selection and
receipt, reconnects, and verifies explicit fallback clears selection. It failed
with a null selection before the fix and passes on Android API 27 and 34.

The host also waits up to 15 seconds for active, matching capture output before
starting its existing bounded candidate windows (six seconds each, 36 seconds
total). Encoder creation alone, wrong geometry and retired output do not certify
readiness. Existing outer cancellation still handles input, peer, mode and
shutdown changes. Missing capture returns to the ordinary unverified fallback.
A permanent paused-clock regression failed when a seven-second helper startup
consumed the first trial, then passed after readiness gating. Timeout and stale
output cases remain in the normal suite.

## Native acceptance

The signed release updated `io.github.geraldo_netto.blent` in place, preserving
app data and its designated signing identity. One ordinary Blent package remains.
A subsequent bounded 1280×800 calibration accepted named-decoder receipts for
software budgets 1, 2 and 4, VAAPI baseline, an unhinted variant and an alternate
Android decoder. No receipt mismatch was observed. Camera clients remained empty;
no additional camera test occurred. Temporary host processes were stopped and
per-tag Android logging properties restored after each observation.

| Software workers | Effective workers | Accepted post-warmup samples | ACK p95 / p99 |
| ---: | ---: | ---: | ---: |
| 1 | 1 | 67 | 18.463 / 22.328 ms |
| 2 | 2 | 67 | 18.491 / 21.732 ms |
| 4 | 4 | 60 | 18.863 / 19.230 ms |

The selector retained one worker. Higher counts rendered successfully but did
not meet the material-improvement guards; no speedup is claimed. Desktop content
was uncontrolled, so these are bounded correctness observations, not a general
performance ranking. Timing starts at packet readiness, ends at receipt of the
render callback ACK and does not measure optical presentation. Final selected
epoch 8 accepted 771 ACKs before normal shutdown.

## Automated verification

All 54 selection tests pass. Host library/application tests pass (three existing
ignored native/environment tests retained). The full Android suite, release
build and release lint pass. All 1,253 Linux Rust functions meet the 80% gate;
changed selector sources were recollected, and only byte-identical files retain
T612 counters, with source-hash attestation. Complexity remains at most nine.
Non-Linux evidence remains explicitly unavailable under existing platform TODOs.
Android has 577/578 methods passing; the unchanged physical-size handshake method
is 7/9 lines and is recorded separately as T629. The changed live decoder method
passes. No regression was removed, skipped or weakened.

[Evidence](artifacts/2026-09-26-task-batch/t627/) includes red/green logs, native
counts and traces, APK verification and coverage. The native-only diagnostic
patch logs rejected receipt identity; it was removed after observation, leaving
the existing rejection behavior and production logging unchanged. Its exact
patch is retained so the extra native trace is reproducible. Selection logging
now includes requested workers and decoder choice. The prior T612 report records
the historical failure; this report resolves its T627 follow-up.
