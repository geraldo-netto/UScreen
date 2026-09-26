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

Current reproduction uses the [temporary profiling runner](../profiling-android.md),
which removes the shell-only test APK afterward:

```sh
./android/gradlew -p android :app:assembleProfile :app:testProfileUnitTest
python3 scripts/benchmarks/profile-session.py --serial DEVICE \
  --package io.github.geraldo_netto.blent.profile \
  --apk android/app/build/outputs/apk/profile/app-profile.apk -- \
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

## T579 — damage-triggered GPU capture

The opt-in Linux helper now accepts `BLENT_GPU_CADENCE=damage`; absence or
`periodic` retains periodic GPU capture, and ordinary Blent still defaults to
FIFO. Native XDamage handling coalesces damage within the owned output, bounds
queue draining, and uses at most 8 ms pointer polling for cursor-only motion.
Idle capture refreshes at 5 FPS, limited by the configured maximum FPS. Cursor
shape changes and motion elsewhere can cause extra captures. No native handles
enter shared UI/configuration/wire contracts.

The timing/rectangle policy has no X11 dependency. Every wait checks that the
previous GPU consumer released its lease. Existing native deadlines and FIFO
fallback remain. Sparse cadence requests an intra frame on a 0.9-second wall-clock
threshold, rather than stretching a frame-count GOP to six seconds.

Three alternating triples at each source rate compare FIFO, periodic GPU and
damage GPU through the same physical USB decoder. Each trial has 180 pictures,
with 30 warmup pictures excluded from latency summaries. Decoded barcodes match
source timestamps; all 3,240 pictures receive their exact render ACK, with a
further 60/60 ACKs in the sparse-scene check. Same stock FFmpeg 6.1.6, QP 18,
1280×800, 30 FPS target, tablet decoder and binary hashes across the paired trials.
GPU capture uses X11's renderD129; FIFO uses the existing renderD128 encoder.

| Source Hz | Path | Median trial p50, ms | Median trial p95, ms | Capture/encoder CPU seconds per elapsed second |
| ---: | --- | ---: | ---: | ---: |
| 29 | FIFO | 56.94 | 59.68 | 0.0880 |
| 29 | Periodic GPU | 37.89 | 52.67 | 0.0467 |
| 29 | Damage GPU | 23.33 | 27.67 | 0.0484 |
| 30 | FIFO | 68.88 | 71.08 | 0.0921 |
| 30 | Periodic GPU | 41.53 | 43.76 | 0.0469 |
| 30 | Damage GPU | 37.98 | 54.70 | 0.0439 |

Damage improves both latency percentiles in all three 29 Hz pairs. At 30 Hz it
improves two p50 pairs but worsens all three p95 pairs against periodic GPU.
There is no consistent all-rate latency gain; keep it opt-in. T593 records the
matched-rate tail follow-up. FIFO timing differs from the historical September
21 setup; these are new paired observations on an active, unpinned workstation,
not a controlled cross-date regression attribution. CPU excludes compositor,
other applications and Android; no battery or optical-latency claim is made.

The 1 Hz scene produces 60 encoded/ACKed frames over 11.71 seconds, maximum
frame gap 201.2 ms. Twelve keyframes have gaps 909.7–1001.3 ms. The temporary
unused monitor and ADB mappings retire; the original display remains attached
and Blent is restored afterward.

Normal Rust sanitizer policy/bounds tests and isolated Xvfb tests pass. All nine
new native policy/event functions have 100% measured executable-line coverage;
strict workspace Clippy passes. The normal benchmark test collection passes
111 tests. Retained [native streams, commands and exact source copies](artifacts/2026-09-26-next-batch/t579-native.tar.gz),
[binary hashes](artifacts/2026-09-26-next-batch/t579-binaries.json),
[29 Hz summary](artifacts/2026-09-26-next-batch/t579-29-summary.json),
[30 Hz summary](artifacts/2026-09-26-next-batch/t579-30-summary.json),
[idle/keyframe timing](artifacts/2026-09-26-next-batch/t579-idle-summary.json),
[coverage](artifacts/2026-09-26-next-batch/t579-coverage.json), and
[GCOV counters](artifacts/2026-09-26-next-batch/t579-gcov.tar.gz).

## T574 — current status versus historical measurements

Historical damage-conversion, idle-writer, raw-input, codec and GPU reports now
identify their measurement date and link implemented behavior/acceptance.
Current commands use Blent identities. Original measurements, unavailable native
hardware, and the distinction between implemented policy and physical acceptance
are preserved. The TODO ledger reflects completed camera coverage and current
package/trace/ownership names. T584 is explicitly declined; Windows obligations
remain, and no macOS application support is claimed.

Local Markdown targets exist, original license/header identity tests pass, and
the final [complexity gate](artifacts/2026-09-26-next-batch/complexity.log.gz) reports
no function above nine. Documentation-only corrections require no artificial tests.
