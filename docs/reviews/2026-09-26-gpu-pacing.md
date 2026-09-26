# T593: pacing, drift and source phase

No production cadence change is promoted. FIFO remains default; damage capture
remains opt-in. Native tests show that removing timer rounding alone is
insufficient, and preserving the target cadence does not remove dependence on
the relative source/capture phase.

## Experiments and retained evidence

The prior damage implementation schedules from each actual capture start and
rounds waits up to milliseconds. Its previous 30 Hz trials averaged about
34.08–34.14 ms per capture, versus the 33.333 ms target. Two development-only
candidates were built from the existing helper:

1. Nanosecond `ppoll` deadlines with the existing relative cadence.
2. The same wait plus a preserved target schedule, resetting after long gaps
   instead of accumulating each wake-up's lateness.

Both copies, build inputs, hashes, scene identities, capture starts, encoded
packet timestamps and physical render ACKs are saved in
[the evidence directory](artifacts/2026-09-26-performance/t593/).
Raw encoded streams and executables also remain in the private archive.

The first candidate's 29/30 Hz trials overlapped Android test/build work on the
host. They are retained as exploratory data, **excluded from performance
acceptance**. The second candidate ran without concurrent build/test workloads,
three alternating periodic/damage pairs at each source rate, 180 encoded frames
per trial and 30 warm-up frames. All 2,160 frames had exact render ACK coverage.
The explicit DRI3 probe ran separately; all these captures use the proven
same-GPU import route and the named `c2.unisoc.avc.decoder`.

## Quiet-window results

At 29 Hz, median-of-trial p50 source-update-to-ACK was 25.62 ms for the candidate
versus 37.67 ms periodic; p95 was 50.59 versus 53.05 ms. Every pair improved,
although this is not the earlier report's roughly 27 ms damage tail.

At 30 Hz, periodic/candidate p95 pairs were:

- 28.72 / 46.56 ms: candidate worse by 17.84 ms.
- 47.16 / 35.21 ms: candidate better by 11.95 ms.
- 37.47 / 30.32 ms: candidate better by 7.15 ms.

The 30 Hz candidate held a 33.334 ms mean capture interval. Its capture-to-packet
p95 was 4.17–5.08 ms; periodic was 4.30–4.57 ms. The source-to-capture p95 pairs
were 4.36/22.32, 22.55/11.69 and 13.33/4.99 ms. The changing source age, rather
than a consistent encode improvement, explains the direction of these pairs.
These are associated within-trial distributions; their separate percentiles must
not be added or subtracted as if they described the same frame.

Source timestamps mark X11 submission, capture timestamps precede native
geometry/copy work, and ACKs are host receipt of Android render callbacks.
None is an optical presentation timestamp. Damage-arrival timestamps were not
instrumented, so this does not isolate every compositor/event-queue delay.

Historical next step at this stage (subsequently completed by the
[controlled phase sweep](2026-09-26-controlled-gpu-phase.md)): explicitly sweep source/capture start phase and record
XDamage arrival alongside the existing frame identities. Any production change
also needs permanent timing/ownership regressions, sparse updates, cursor-only
motion and no catch-up bursts. The schedule-preserving candidate changes the
strict inter-frame spacing contract and cannot be shipped solely because its
average FPS looks correct.
