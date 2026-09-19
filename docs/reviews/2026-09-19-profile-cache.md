# T480 — optional measured profile persistence

Linux Video settings now expose **Reuse a recent measured profile**, off by
default. The normal FFmpeg CLI selector can reuse one recent historical winner
after fresh host probes, peer compatibility and matching render receipts. It
never migrates a manual encoder choice. See the [user contract and invalidation
scope](../video-codecs.md#optional-historical-profile-cache).

The cache separates stable identity/software/format/transport context from
attachment generations and negotiation scopes. The attachment lease captures
identity and route atomically; retired leases cannot authorize reuse or writes.
Android adds a bounded optional firmware/source-build fingerprint to protocol 2,
with distinct debug/release inputs. Older reports remain valid but cannot use
persistent profiles. Generated fingerprint code and all cache state stay out of
Git; no token or raw tablet identity is saved in the cache record.

Persistence uses an owner-only temporary file, fsync and atomic replacement.
Malformed, oversized, stale and incompatible records fall back to measurement.
The cached result retains its original timestamp. Failed fresh verification or
subsequent render progress invalidates it; the selector resumes measurements.
The global probe admission permit is released during healthy cached operation.
The report distinguishes historical ACK timing from current host-probe quality.
This does not establish a new latency or battery gain.

## Permanent automated evidence

Added tests first reproduced missing opt-in persistence, ignored malformed
software identity, and Android's absent software field. The same assertions now
pass. New cache and selection-policy tests cover:

- Private persistence, interrupted writes, malformed/oversized/symlinked files,
  future timestamps, expiry boundaries and unknown schema fields/types.
- Stable identity across transient scope changes; changed identity, transport,
  software/host/format context; retired attachment leases and manual preferences.
- Fresh host capacity/quality/profile checks, exact decoder/hint compatibility,
  matching render verification and historical status wording.
- Repeated invalid/out-of-bounds observation fields and 512 deterministic byte
  mutations. Android fingerprints also exercise empty, Unicode/NUL and long input.
- A live-policy simulation where the cached profile verifies, releases probe
  admission, then stalls: its record is discarded and current measurements run.
  Input interruption preserves fallback without claiming success.

Validation: 414 Linux daemon tests passed (three existing ignored tests), 69
shared unit tests plus platform/integration checks, one further focused record-schema
regression, 52 GUI unit tests plus the
isolated startup test, and 379 Android tests. Debug APK assembly and release
Kotlin compilation passed with distinct generated variant identities. Clippy
passed for both normal and all-feature workspace builds; MSVC cross-check passed.
Formatting passed; 4,221 functions scanned with no complexity above nine.

The 26 new production Rust functions/methods have **85.71–100% executable-line
coverage**; cache and preparation modules individually range from 94.12–100%.
JaCoCo measures 100% line coverage for the new Android fingerprint method and
its formatting lambda, and for the updated scoped-capability method. These are
scoped results, not project-wide compliance with T497.

Raw red/green logs, LCOV, JaCoCo XML, scoped function inventory and the JaCoCo
initialization script are [archived here](artifacts/2026-09-19-profile-cache/).
Rust coverage uses `cargo llvm-cov` with the selection tests and the T480 shared
and GUI tests; merge line hits across those reports. Android coverage uses
JaCoCo 0.8.12 and the normal `testDebugUnitTest` suite through the archived init
script's `profileCoverage` task. No tablet, active desktop capture, deployment,
release publication or remote push was involved.
