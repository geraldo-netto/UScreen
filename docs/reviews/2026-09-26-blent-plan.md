# Blent rename, application updates and allocation review

Planning snapshot, 2026-09-26. Tracked by T585–T587 in [TODO.md](../../TODO.md).
The rename, application updates and memory changes are proposed work; this
review changes no runtime behavior and provides no new performance measurements.

## Selected scope

The maintainer selected rebranding, rebuilding and deploying both applications,
with new Blent identities, no migration and no compatibility with past versions
or APIs. Remove links to the original UScreen repository while retaining its
author's copyright and license notices. Feature additions and dependency or
toolchain upgrades are outside this scope.

Use `Blent` for visible branding, `blent`/`blent-gui` for commands, `blent-*`
for release artifacts, `BLENT_*` for environment variables and Blent-specific
config/data/runtime/service names. Proposed Android application ID:
`io.github.geraldo_netto.blent`; namespace: `com.blent`. Keep the existing
protected signing key unless a technical requirement demands otherwise; a new
application ID does not require replacing that key. Start with fresh Blent
settings. Do not add old command aliases, path fallbacks or migration code.

Android currently uses `io.github.geraldo_netto.uscreen`, independently of the
`com.uscreen` Kotlin namespace. Host component construction is centralized in
`common/src/android.rs`; signing and artifact verification use the documented
[fork identity](../release-signing.md). Its old-ID continuity instruction is
superseded by the maintainer's decision and must be updated with the code. A new
application ID is a different app; deployment must explicitly launch Blent.
See [Android's application-ID contract](https://developer.android.com/build/configure-app-module).

## Implementation order

1. **T585: inventory and attribution.** Classify every old-name reference as
   current branding, compatibility contract, upstream attribution, third-party
   material or historical evidence. Keep `LICENSE`, including
   `Copyright (c) 2026 majmichu1`, and original file notices. Preserve DisplayLink
   notices in `host/evdi/evdi_lib.h`, `THIRD_PARTY_LICENSES.md`, bundled dependency
   notices and matching source-distribution assets. Keep copyright attribution
   as plain text and remove original UScreen repository hyperlinks from current
   project material. Retain required third-party license/source references.
   Historical claims must remain clearly historical, with plain source labels
   where links are removed; do not relabel old results as Blent measurements.
   Verify notices in built distributions as well as source; retain immutable
   logs and signed evidence.
2. **T585: apply the selected identity policy.** Update Rust package/binary names,
   Linux GUI/tray/desktop/service integration, package recipes and installers,
   Android labels/themes/build naming, host discovery, release scripts, CI and
   current documentation together. Namespace changes must update generated
   Kotlin, component names and tests. Update project-owned wire identifiers
   consistently at both endpoints; no legacy protocol adapter is required.
   Centralize shared identity logic;
   OS paths and lifecycle stay inside backend adapters. Inspect Git remotes and
   update links only against a confirmed repository destination; a hosted repo
   rename or release publication is a separate action to specify.
3. **T586: build and validate the selected update scope.** Produce matching Linux
   artifacts and signed Android APK; verify version metadata, certificate,
   application ID, launcher, notices and source bundles. Exercise permanent
   installer/upgrade, settings, autostart/single-instance, discovery and reconnect
   tests using fresh Blent settings and consistent endpoint identities. Retain
   rollback artifacts, stop old active application/service/autostart ownership,
   deploy both builds and explicitly launch Blent. Check display/input and
   enabled camera/audio paths on the available Linux/Android setup, including
   restart/reconnect and absence of duplicate owners. Do not copy old settings;
   retain unrelated data. Record unavailable native checks.
4. **T587: measure, then optimize.** Baseline review can start independently of
   branding. Benchmark changes separately from the rename so measurements can
   isolate allocation policy. Select changes only after comparing memory use,
   allocation churn, CPU and latency under the same workload.

For confirmed bugs, add permanent TODO-linked regressions first, demonstrate
failure, then fix and demonstrate success. Retain all existing regressions.
Enforce the repository's per-function 80% executable-line coverage, bounded
invalid-input fuzzing and cyclomatic-complexity maximum of nine for maintained
production code. Track unavailable validation and existing gaps separately;
T572 already records Linux camera coverage gaps. Commit each resolved finding
separately and remove only its resolved ledger row. Planning itself requires
no artificial runtime tests.

## Allocation evidence already available

- **Historical Rust packetizer work:** [T384 results](../benchmarks.md#annex-b-packetizer-replay)
  report H.264 fragmented replay allocations falling from 19,191 to 390.
  These are synthetic replay counts, excluding separately counted reallocations;
  they do not establish current whole-application memory or latency gains.
  Current source retains allocation probes and T402/T407/T416 replay harnesses.
  Some older Annex B input-buffer paths are now test-only, so use current
  production framing for the next baseline.
- **Current Android reader:** `VideoPacketReader.kt` starts with 512 KiB and,
  only when necessary, allocates `size + size / 2` after validating packet size.
  It reuses the buffer and dispatches borrowed slices; capacity persists until
  reader retirement. This can retain headroom after a large packet, but does
  not allocate a fresh payload for every ordinary frame. Compare retention and
  growth frequency before changing its policy.
- **Current host storage:** `host/src/media_storage.rs` charges a Vec's capacity,
  retaining that charge across shared slices. `host/src/ivf.rs` validates the
  advertised size before allocating a frame-sized Vec. The C raw ring derives
  storage from geometry and slot count; capture framebuffer allocation rounds
  up to 2 MiB alignment. Alignment, padding and simultaneously live buffers
  belong in retained-memory accounting.
- **Android measurement gap:** [T560 live observations](../benchmarks/2026-09-20-live-playback/README.md)
  captured heap size and scheduling, but no allocation rate/call stacks.
  [Earlier Android experiments](2026-09-20-android-performance.md) measured
  allocation differences between decoder modes; those isolated results do not
  replace a current application allocation profile.

## Proposed allocation comparison

Inventory Rust Vec/Bytes, C buffers/rings and Android arrays/codec ownership.
For each, record size source, validation limit, growth policy, backing capacity,
copy sites, concurrent owners and release boundary. Distinguish Rust allocator
counters from C/FFmpeg allocations, mapped memory and Android managed/native
storage; requested bytes and RSS are not interchangeable measurements.

Compare known-size preallocation, current bounded geometric growth, measured
size-class reuse and trimming at safe lifecycle boundaries. Exact sizing is a
good candidate for completed, validated messages and fixed geometry. Variable
streams may benefit from retained spare capacity; growing by tiny increments
or shrinking every frame can increase allocator work. Rust's
[`reserve_exact`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.reserve_exact)
does not guarantee exact physical allocation, and cannot remove existing spare
capacity by itself.

Use fixed single-device traces plus isolated replay: steady frames, a large
keyframe followed by small frames, increasing/decreasing resolution, slow
consumers, reconnect and malformed/oversized lengths. Measure allocation and
reallocation counts, copy bytes, live/peak retained storage, GC pauses, CPU and
p50/p95/p99 latency; record instrumentation overhead. Preserve packet bytes,
ordering, bounds, cancellation and final-consumer ownership. Keep capacity
choices configurable where they affect user tradeoffs, with documented defaults.
Do not reopen T382's declined multi-tablet/large-machine campaign.
