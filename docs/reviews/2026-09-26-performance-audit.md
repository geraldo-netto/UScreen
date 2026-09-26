# Performance audit: host and Android

The implemented win is [T594 bounded camera packet staging](2026-09-26-camera-staging.md):
about 99.94% less process allocation and 35% less elapsed time in the bounded
synthetic camera-write workload. The signed Android release was rebuilt,
certificate/package verified, installed and reopened. Camera capture stayed off.

[T578](2026-09-26-gpu-layout.md) found an actual DRI3 prerequisite; it remains
blocked. [T593](2026-09-26-gpu-pacing.md) narrowed tail latency to source/capture
phase and retained unsuccessful candidates; it remains open. Neither GPU
experiment changes the production default. [T597](2026-09-26-algorithm-costs.md)
records a theoretical algorithm improvement whose tiny practical benefit does
not justify a production change.

## Reusable profiles

Open the SVGs in a browser; search and click frames to inspect them. Width is
accumulated sampled CPU time, except the explicitly labeled off-CPU graph.
These are aggregates, not chronological timelines.

- [Host, idle](artifacts/2026-09-26-performance/t596/graphs/idle-steady-host.svg)
- [Host, moving scene](artifacts/2026-09-26-performance/t596/graphs/motion-steady-host.svg)
- [Android full client, idle](artifacts/2026-09-26-performance/t596/graphs/idle-steady-android.svg)
- [Android full client, moving scene](artifacts/2026-09-26-performance/t596/graphs/motion-steady-android.svg)
- [Android decoder replay, off-CPU](artifacts/2026-09-26-performance/t596/graphs/android-offcpu.svg)

Static previews: [host](artifacts/2026-09-26-performance/t596/graphs/motion-steady-host.png) and [Android](artifacts/2026-09-26-performance/t596/graphs/motion-steady-android.png). Generated from the matching SVGs using CairoSVG 2.9.1.

### Workload and boundaries

The Linux host streamed its real 1280×800 EVDI display through the normal FIFO,
stock FFmpeg and ADB route. Steady trials used `libx264`, configured for 60 FPS;
the moving X11 barcode scene updated at 30 Hz. Android ran the full production
client sources in the separately installed **debuggable profile variant**.
No synthetic-only allocation workload substitutes for these client profiles.
This build's ART/debug/JNI overhead prevents interpreting its absolute CPU cost
as the optimized signed release's cost. Its source/build/APK identity is retained
in the allocation-baseline metadata and source archive.

Each CPU window lasted 20 seconds at 199 Hz sampling. Separate ten-second
`strace -f -c -w` windows followed; their overhead does not contaminate the CPU
windows. Host scope is the daemon, EVDI helper, encoder and ADB process, not all
Xorg/compositor/desktop work. Android scope is the app process, not vendor codec
services or SurfaceFlinger. Codec auto-selection overlapped the first exploratory
idle capture; that capture is retained privately and excluded from steady results.
The retained steady campaign waits 65 seconds after entering the profile client.

The separate 15-second Android off-CPU recording used the instrumented native
USB decoder replay with the named `c2.unisoc.avc.decoder`, a 29 Hz source and
same-GPU periodic encoder. All 600 replay frames received render ACKs. That graph
covers decoder/transport waiting, not the complete Compose/control client.

### CPU observations

Host sampled CPU totals were 0.548 seconds idle and 2.683 seconds moving over
20-second windows: approximately 0.027 and 0.134 CPU cores across the selected
processes. These are sampling estimates, not power measurements. The moving
window contains 534 samples; fine-grained sub-percent rankings are not reliable.

About half the moving host samples land in libx264 code, including stripped
internal functions. The exported label `x264_8_trellis_coefn` is not a reliable
attribution of every nearby assembly instruction to trellis work. Other notable
self samples are EVDI `convert_job` (~8.8%), kernel `_copy_to_user` (~11.6%),
`drm_clflush_virt_range` (~5.8%) and `find_vmap_area` (~5.1%). These support
investigating capture readback/copy boundaries before rewriting tiny Rust loops.
They do not establish that replacing a syscall by itself will reduce latency.

Initially restricted kernel maps were resolved afterward using a saved text-
symbol map from the same boot. The renderer records its explicit address lookup
rule; raw maps remain private. Host callchains are unavailable for 58/109 idle
samples and 269/534 moving samples. The sampled instruction is preserved under
`[callchain unavailable]`; unresolved user symbols retain their library name.
Do not treat a missing stack as an empty-cost function or claim full attribution.

Android full-client sampled CPU totals were 3.533 seconds idle and 8.613 seconds
moving per 20 seconds, approximately 0.177 and 0.431 cores. Codec/Binder/buffer
management, JNI, transport and callback threads dominate the retained graphs.
Absolute release overhead needs a matching optimized, shell-profileable build
(T598). No sustained battery conclusion follows from these short runs.

## Syscalls and locks

In the moving host's separate ten-second wall-time trace, accumulated syscall
elapsed time was approximately:

- `futex`: 316.64 seconds, 84.08% of summed syscall time, 119,013 calls.
- `epoll_wait`: 19.79 seconds, 5.26%.
- `read`: 10.54 seconds, 2.80%.
- `ioctl`: 10.24 seconds, 2.72%.
- `poll`: 9.50 seconds, 2.52%.
- `clock_nanosleep`: 9.07 seconds, 2.41%.

Totals exceed wall time because many threads wait concurrently. `strace -w`
measures elapsed syscall duration, including sleep, and perturbs scheduling.
It is **not CPU usage or serialized application stall time**. Summed futex wait
alone is not proof of a contended application mutex.

The Android off-CPU trace totals 195.28 thread-seconds over 14.945 seconds. Top
sampled leaf groups include futex waits 85.57 seconds, `ioctl` 31.37 seconds,
Looper polling 18.73 seconds, `usleep` 18.00 seconds and socket `poll` 13.23
seconds. The large stacks are codec condition variables, idle Binder pool
threads, buffer-pool eviction sleep, frame-callback Looper waiting and the replay
producer waiting for packets. The supplied simpleperf tracepoint inventory did
not expose raw syscall enter/exit events, so these are blocked-stack durations,
not a complete Android per-syscall count/duration census.

Static ownership review finds real locks: host latency history, video admission,
backing-storage budgets and attachment state; Android `FrameTiming`,
`DecoderSession`, `CodecLifetime`, mailbox and control-session state. Capture
conversion workers use conditions and short state locks around publication;
pixel conversion runs outside the pool mutex. Android codec borrowing increments
ownership under a lock but performs native input work outside it. Host report
sorting/logging takes place after swapping samples out of the state lock.

No application-lock bottleneck was demonstrated. The off-CPU stack search found
no attributed monitor/mutex-acquisition hotspot; incomplete stacks, unprofiled
processes and this single workload prevent a universal no-contention claim.
Preserve ownership locks. If future traces identify a blocked critical path,
measure that lock's wait and hold distributions before narrowing its scope or
changing data ownership. Lock-free replacement would add correctness risk without
an established benefit here.

## Caches and reuse

There is no single useful whole-application hit ratio; the mechanisms have
different lifetimes and denominators.

- Android `FrameTiming` has a direct tag lookup backed by bounded history.
  Added **benchmark-only** hit/miss instrumentation measured 5,520 hits, zero
  misses over 2,760 rendered frames in 13 native replay trials. That is 100% for
  these sequential runs, not proof about collision/reorder/reconnect workloads.
  Existing collision and epoch regressions remain necessary.
- Host persistent codec-profile selection is explicitly opt-in and currently
  disabled (`profile_cache = false`). Hit rate is **not applicable**, not zero.
  Disabling it spends more startup/reconnect work selecting a profile; it does
  not cause a per-frame miss. Reuse validates attachment/software/settings and
  still checks the selected route. Configuration was preserved.
- Codec configuration/parameter sets retain immutable shared storage and avoid
  rebuilding unchanged headers. Their hit ratio was not instrumented here.
- Android packet-reader capacity, EVDI frame buffers, conversion workers, raw
  frame slots and GPU NV12 surfaces are reusable storage/resources, not key/value
  caches. Steady reader reuse already showed zero backing-array replacements in
  T589; that metric should not be mislabeled a cache hit ratio.
- EDIDs use versioned file reuse; daemon process identity is revalidated; KWin
  caches successful backend discovery. GUI capability probes have a ten-second
  TTL while dynamic status stays fresh. No live hit-rate counters were collected
  for these cold/control paths, and GUI was outside this CPU sampling scope.

## Algorithms, structures, loops and strings

The retained inventory identifies **1,602 maintained functions in 170 source
files**, including portable/native application code and essential scripts. It
is a source/contract inventory, not a proof that every function is optimal or
that unavailable Windows/macOS paths received native profiling. The detailed
static review focused on the following pipeline families:

| Family | Scaling and current design | Assessment |
| --- | --- | --- |
| EVDI conversion | O(H + damaged source pixels); bounded scale and worker count; span/dirty-row scans plus conversion | Nested pixel loops are not automatically quadratic in frame size. Reducing full-frame readback/copy matters more than changing loop syntax. |
| Complete-packet parsing | O(B) NAL scan and bounded byte copies for B encoded bytes | Necessary byte inspection remains linear. The incremental assembly helpers gated by `cfg(test)` must not be mistaken for the active tee packet path. |
| Stream fanout | O(C) clients per packet; shared immutable payloads; eight queued packets, 32 MiB storage budget, bounded client count | Avoids cloning every payload. Backlog trimming protects latency; larger queues are not automatically faster. |
| Host ACK lookup | O(1) contiguous sequence lookup; bounded O(Q) fallback for discontinuities, Q ≤ 256 | Extra indexing has little justification without frequent fallback evidence. |
| Android timing lookup | O(1) cache hit, bounded O(R) fallback; fixed ring | 100% measured hit ratio for the sequential native cohort. |
| Camera packet write | O(B) bytes, now bounded segment staging | T594 removes allocation volume and retained staging without pretending the required copy disappeared. |
| Latency percentiles | O(n log n), n ≤ 1,024, about once per five seconds | T597 selection candidate has lower worst-case complexity but input-dependent constants and negligible live benefit. |
| Discovery/selection/configuration | Inventory scans, small codec cross-products, JSON/TOML and command construction | Mostly startup/control work. A faster startup strategy is measured-profile reuse, not blanket removal of validation or string allocations. |

FIFO/raw-frame transfer, display packet reads and codec input necessarily process
their bytes. Existing geometric bounded growth amortizes reader allocation;
exact-sizing every packet would recreate the allocation churn T594 removes.
Configuration and control messages do allocate strings/JSON, and logs format
periodic summaries, but this profile does not identify string operations as a
hotspot. A hand-written binary control protocol or extra string cache would be
an unmeasured wire/lifecycle change. Keep bounded `VecDeque`/`ArrayDeque` queues
and rings; no measured reason supports a wholesale map/list replacement.

## Zero-copy and transport

Several copy reductions already exist:

1. Host shared immutable encoded payloads and vectored frame-header/payload writes
   avoid an extra concatenation buffer; kernel socket/ADB copies still exist.
2. Optional in-process encoding can fill encoder planes directly, and the shared
   NV12 ring avoids a FIFO boundary. Neither removes EVDI's RGB readback or all
   encoder-internal copies.
3. Experimental same-GPU DRI3/VAAPI capture keeps the RGB import/conversion on the
   GPU, with explicit lease completion. The helper still retains EVDI lifecycle
   work; safe suppression of redundant readback needs a separate measured change
   (T599). Cross-device layout remains blocked by T578.
4. Android `ChannelPacketReader` can read into codec-owned buffers. The prior
   [T403 device comparison](../benchmarks/2026-09-18-decoder-input.md) found no
   consistent CPU/tail benefit and retained the reusable heap path. A direct
   `ByteBuffer` is not DMA or kernel zero-copy. Do not reopen that result based
   only on counting copies.
5. T594 removes camera payload arrays but still copies into Okio segments and
   the socket. It creates no camera sensor activity during profiling.

The USB link still enumerates at 480 Mb/s. Application display traffic travels
through TCP loopback into ADB, so loopback `ss` RTT/throughput estimates do not
measure the USB wire. Existing `TCP_NODELAY`, vectored writes, bounded send
buffers, frame/backlog limits and deadlines are appropriate latency controls.
Blindly enlarging buffers can hide congestion by accumulating older frames.
Twelve one-second motion snapshots showed zero host loopback send/receive
queue bytes and about 0.315 Mb/s sent over that simple barcode scene. This is
a low-complexity workload, not a saturated-link test; the snapshot series
cannot rule out brief queues between samples. The retained network observations
are a bounded display-only sample; no Wi-Fi
comparison, combined camera load, link bonding or USB 3 capability is established.
T553 remains the combined-routing research item; the explicit camera-off request
precludes silently enabling a camera workload for it.

## What to do next

1. Keep the measured T594 improvement; preserve camera consent and lifecycle.
2. Finish T593's controlled phase/event investigation before promoting GPU cadence.
3. T598: obtain an optimized shell-profileable full-client baseline and improve
   native symbol coverage before assigning fine-grained release CPU/lock costs.
4. T599: evaluate suppressing redundant EVDI readback only under an admitted GPU
   consumer, with ownership-safe fallback and complete render validation.
5. Leave T597's microsecond reporting change deferred; preserve T578's real
   prerequisite and T553's explicit routing scope.

## Preservation and validation

Reviewed summaries, folded stacks, five SVGs, workload/renderer source snapshots,
build identities and checksums live in
[the versioned evidence](artifacts/2026-09-26-performance/).
All raw traces, same-boot kernel symbols, logs, encoded test streams, candidate
sources/binaries and APKs are retained under:

`/home/netto/.local/share/blent/profiles/2026-09-26-performance/`

That directory is owner-only. Raw DWARF stack dumps can contain process memory;
they are intentionally retained locally rather than added to Git. No signing
password, private key, runtime authentication token or keystore properties file
is part of the archive. `SHA256SUMS` and the manifest identify artifacts; source
IDs distinguish the measured profile builds from the installed signed release.
The source snapshots record exact commands and local prerequisites; reconstruct
paths from the archive when replaying them on another machine.

Android: 526 unit tests, changed writer 100% executable-line coverage, release
build, lint and signing/installed-hash verification pass. Existing 111 benchmark
Python tests pass. Cyclomatic validation reports no function above 9; this is
separate from algorithmic complexity. SVG XML, JSON and checksums were checked.
Linux host production code did not change in this audit; no Linux application
redeployment was needed. Main Android Blent is restored, temporary test output is
off, ADB reverse mappings and host configuration are preserved, camera is off.
