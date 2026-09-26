# T594: bounded camera packet staging

`CameraWire.packet` writes directly from a duplicate `ByteBuffer` view into
8 KiB Okio segments, emitting completed segments before copying more payload.
The caller's position, limit and mark remain unchanged. The 2 MiB packet cap,
big-endian length prefix, packet flush, socket timeout and ownership rules stay
intact. This removes the per-packet payload array without adding a second pool,
shared mutable scratch storage or a camera-lifecycle cache.

A bare `sink.write(wholeByteBuffer)` would still accumulate the whole payload
before emitting segments; the explicit 8 KiB limit is necessary. This remains a
copy into buffered transport storage, not end-to-end zero-copy. A retaining
in-memory `Buffer` can deliberately retain all output; staging is bounded when
using the production streaming `BufferedSink` and its consuming transport.

## Native comparison

RugKing Pad 2 Pro, Android API 36; three fresh-process trials per implementation.
The existing T589 profile activity writes 1,200 synthetic packets into a blackhole
sink: 40 × 1 MiB and 1,160 × 64 KiB, totaling 117,964,800 payload bytes.
**No camera sensor, encoder or actual camera transport was enabled.**

- Original process allocation: 184,721,408–184,836,096 bytes; median 184,803,328.
- Segmented process allocation: 106,496–131,072 bytes; median 106,496, a 99.94%
  reduction in this workload. Counters are process-wide ART measurements,
  published at GC boundaries, not an attribution of every byte to this method.
- Original elapsed: 340.7–365.0 ms; median 353.4 ms.
- Segmented elapsed: 228.3–228.4 ms; median 228.3 ms, about 35% lower.
- Original workload GC: 3–6 collections, 77–118 ms aggregate GC time.
  Candidate: zero reported workload collections. Forced settling collections
  remain separate; no claim of zero allocation or physical battery gain.

The same seven-workload harness ran for both builds. Source IDs, APK hashes,
firmware, raw counters and summaries are retained in
[the T594 evidence](artifacts/2026-09-26-performance/t594/).

## Permanent validation

`CameraPacketBufferTest` checks exact wire bytes at segment boundaries and the
2 MiB limit, heap/direct/read-only buffers, preserved caller state, bounded
emissions, closure of a blocked slow sink and a fresh session without stale bytes.
The bounded-emission regression fails against the retained original writer;
the same test passes after the change. Existing bounds fuzzing, handshake,
retirement and camera-capture tests remain in the normal suite.

All 526 debug unit tests pass. JaCoCo measures `packet` 10/10 executable lines,
`connect` 14/14 and `greeting` 4/4. The native profile APK contains the candidate;
the installed signed main application was unchanged during the synthetic trials.
After validation, the signed release was rebuilt, verified against the designated
certificate and installed in place; the device APK hash matches the build.
[Deployment identity](artifacts/2026-09-26-performance/t596/manifest.json) is retained.
Physical camera capture remains off unless explicitly requested.
