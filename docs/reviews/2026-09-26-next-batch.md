# Blent follow-up implementation and validation

## T571 — replay completion

ACK obligations are enqueued before publishing render counts. Completion requires
an active session, every expected render and no outstanding ACK writes; reaching
the drain deadline alone is not success. The deadline remains 750 ms.

Permanent `UsbReplayTest` regressions hold the render-statistics monitor while
observing ACK publication, and exercise missing-render and undrained-ACK timeouts.
Both regressions fail against the original implementation on Android API 27 and
34; all six replay test executions pass after the fix. The tests were written
after implementation, as requested, then run against retained original source.
Existing restart/receipt coverage remains unchanged.

Raw [old-behavior failure](artifacts/2026-09-26-next-batch/t571-red.log.gz) and
[passing validation](artifacts/2026-09-26-next-batch/t571-green.log.gz) are retained.

## T576 — reproducible conversion replay builds

Both replay generators copy every C header from the selected revision, with the
conversion/frame-exchange sources unchanged. Conversion manifests hash every
copied file. Harness-only adapters handle generation-aware publication and use
named job fields, preserving pre-span historical baselines.

Permanent `scripts/tests/test_conversion_sources.py` clean-build tests cover the
working tree, HEAD and pre-header revision `d1de64a`, verify copied bytes/hashes,
and compare row/span replay checksums. Both tests pass; the same tests fail with
the original generators/harnesses. Retained [failure](artifacts/2026-09-26-next-batch/t576-red.log.gz)
and [passing logs](artifacts/2026-09-26-next-batch/t576-green.log.gz).

## T572 — native camera ownership and cleanup coverage

The normal Rust suite now creates fresh PTY character devices, binds fixture
labels over `/sys/dev/char` in a private child mount namespace, and runs the
unchanged native camera adapter with private fake ADB/FFmpeg executables.
It verifies successful device opening, exclusive locks, wrong labels, symlink
rejection, graceful stop, producer failure, reverse-mapping cleanup and release
of both output locks. No live webcam or real ADB mapping is accessed.

All 17 camera tests pass. `open_device` reaches **28/28 executable lines (100%)**;
`run_native` reaches **27/28 (96.43%)**. The remaining line logs a failed cleanup
attempt. [Counters](artifacts/2026-09-26-next-batch/t572-coverage.json),
[LCOV](artifacts/2026-09-26-next-batch/t572.lcov.gz),
[suite log](artifacts/2026-09-26-next-batch/t572.log.gz), and
[ordinary-user native run](artifacts/2026-09-26-next-batch/t572-user.log.gz) are retained.

The fixture requires working Linux user/mount namespaces. Ordinary-user execution
passes on the development host. Container CI grants SYS_ADMIN and disables its
mount-blocking AppArmor profile only for the regression container; namespace
setup failure fails the test explicitly. No production behavior was changed.

## T589 — Android packet allocation profiling

`assembleProfile` builds `io.github.geraldo_netto.blent.profile`, a separate,
explicitly debuggable/profileable development application. Its shell-protected
activity runs production `VideoPacketReader` and `CameraWire.packet` with bounded
synthetic payloads; development sources live under `scripts/benchmarks` and do
not enter release builds. No main-app setting, signing identity or system power
setting changes. The normal release remains installed.

Reproduce with:

```sh
./android/gradlew -p android :app:assembleProfile :app:testProfileUnitTest
adb -s DEVICE install -r android/app/build/outputs/apk/profile/app-profile.apk
python3 scripts/benchmarks/android-allocations.py --serial DEVICE --output /tmp/new-allocation-run
```

Three fresh-process trials ran on the connected RugKing tablet. Every trial
measures steady packets, keyframe-sized packets, changing packet sizes, maximum
legal packets, a delayed consumer, reader recreation and camera packet writes.
The first five cases contain 300 packets; recreation and camera contain 1,200.
The harness observes backing-array identities/capacity and ART process counters.

| Reader workload | Storage replacements after initial buffer | Retained capacity |
| --- | ---: | ---: |
| Steady 64 KiB | 0 | 524,288 bytes |
| 2 MiB keyframe followed by 64 KiB | 1 | 3,145,728 bytes |
| 64 KiB / 512 KiB / 2 MiB resize sequence | 1 | 3,145,728 bytes |
| Maximum legal packet followed by 64 KiB | 1 | 8,388,609 bytes |
| Slow consumer, 64 KiB packets | 0 | 524,288 bytes |
| Recreate reader for every packet | 1,200 | 524,288 bytes in final reader |

Array identities/capacities match across all trials. Recreation requests roughly
634.6–635.2 MB of process allocations and reports 169–254 ms aggregate blocking
GC time. This is an intentionally extreme recreation workload, not observed
network reconnect frequency or evidence of a streaming leak.

Camera writes send exactly 117,964,800 payload bytes/trial. Process allocation
counters report 184.6–185.1 MB, with 1–8 collections and 54–277 ms aggregate GC
time; reported blocking-GC time is zero. `CameraWire.packet` creates one payload
array and one duplicate view per call; Okio and harness/process work also
contribute. This motivates the bounded scratch-reuse comparison in T594, rather
than attributing every allocated byte to one array or claiming a pooling gain.

ART counters are process-wide, can update in batches, and expose aggregate GC
time rather than individual pause distributions or complete object-allocation
stacks. Explicit GC requests before/after workloads are recorded separately;
Android may ignore them. Zero counter movement is not proof of zero allocation.
These are production packet-path workloads on ART, not real camera-sensor,
MediaCodec, live resize, network or end-to-end latency measurements. Keep existing
reader reuse and its legal-size cap; no additional trimming/pooling is enabled.

Permanent profile framing tests pass on API 27/34. Python tests preserve unknown
and reset counters. The broader Android run passed 524 tests and lint; subsequent
T571 extraction passed its six focused executions. Retained
[native observations](artifacts/2026-09-26-next-batch/t589-native.tar.gz),
[summary](artifacts/2026-09-26-next-batch/t589-summary.json),
[device/APK identity](artifacts/2026-09-26-next-batch/t589-metadata.json), and
[build/test log](artifacts/2026-09-26-next-batch/android-final.log.gz).

## T595 — screen timeout during profiling

The initial profiling activity lacked `FLAG_KEEP_SCREEN_ON`. Android reported
`mLastSleepReason=timeout` with its unchanged 15,000 ms screen timeout. Normal
Blent and the decoder replay already set the flag; window wakefulness does not
transfer to a different foreground activity automatically.

The profile activity now holds the flag for its visible window. A permanent
API 27/34 test covers creation and recreation, with workload execution replaced
by an inert test subclass. Both executions fail with the flag removed; all four
profile test executions pass with it restored. The corrected APK is deployed.

Three native profile runs completed across 28.6 seconds with `Awake` before and
after, identical last-sleep timestamps, the same 15-second system timeout, and
normal Blent visible afterward. This verifies no intervening sleep. It does not
attribute the older T549 `force_suspend` report, which remains unresolved.
No system timeout, lock-screen or global stay-awake preference was changed.

Evidence: [regression failure](artifacts/2026-09-26-next-batch/t595-red.log.gz),
[passing profile tests](artifacts/2026-09-26-next-batch/t595-green.log.gz), and
[native power/activity/APK readback](artifacts/2026-09-26-next-batch/t595-native.json).
