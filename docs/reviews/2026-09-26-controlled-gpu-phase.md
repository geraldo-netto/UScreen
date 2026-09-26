# T593: controlled phase rejects the cadence candidate

Research complete: reject the phase-preserving candidate. Keep production
pacing, FIFO default and damage capture opt-in. No implementation promoted.
The [previous quiet-window experiment](2026-09-26-gpu-pacing.md) mixed source
phase with policy; the controlled sweep separates those effects.

## Method

At the tablet's 1280×800 size, align scene submission to a shared monotonic
30 Hz period. Align initial capture to offsets 0, 8.333, 16.667 and 25 ms.
Use the same compiled helper for periodic control and phase-preserving damage
candidate, alternating their order across three repetitions per phase. Capture
120 frames each; exclude 30 warmup frames from latency distributions.
All **2,880 frames received physical Android render ACKs**. Camera stayed off.

Development-only sources add shared-epoch alignment and log XDamage event dequeue
host nanoseconds, server timestamps and whether damage intersects the display.
The latter is client observation, **not server generation time**. Production
sources stay unchanged. No concurrent compilation/test campaign ran in these
windows. A first missing-library launch failed before measurement; it is retained
privately and excluded. The corrected build uses the same bundled FFmpeg SDK
as the prior probe, not the container's distro FFmpeg.

## Results

Source submission to host receipt of Android render callback, p95 milliseconds:

| Start phase | Periodic: repetitions 1 / 2 / 3 | Candidate: repetitions 1 / 2 / 3 |
|---|---|---|
| 0 ms | 25.54 / 24.90 / 24.95 | 25.92 / 24.72 / 25.22 |
| 8.333 ms | 34.36 / 31.95 / 31.80 | 35.92 / 35.14 / 39.90 |
| 16.667 ms | 41.28 / 43.36 / 41.40 | 42.89 / 44.23 / 44.37 |
| 25 ms | 49.06 / 50.37 / 49.91 | 49.28 / 49.20 / 49.38 |

The phase changes tails by approximately 25 ms. The proposed policy has no
consistent improvement under matched phase. The 8.333 and 16.667 ms groups
regress in every pair. Within the measured steady window, the candidate's
minimum inter-capture interval is **4.519 ms**, violating strict 33.333 ms
spacing. Logical-deadline catch-up is therefore unsuitable for this contract,
even when average FPS looks correct. Periodic control also has ordinary timer
jitter; the candidate's large short-interval catch-up is the rejected change.

For intersecting damage events, dequeue-time distance from the most recent
source submission has per-trial p95 0.19–0.33 ms. This proximity measurement is
not an exact event/frame association: XDamage coalesces regions and server and
host timestamps have different clocks. Combined with the retained source frame
identities, it supports the narrower conclusion that waiting for capture phase,
not a consistently slow event dequeue, explains this experiment's tails.
No claim about optical presentation timing follows from these ACKs.

## Decision and retained evidence

T593 closes with a measured rejection, not a shipping optimization. Sparse
updates, cursor-only motion, strict spacing and ownership regressions remain
mandatory **if a different cadence implementation is proposed later**; no tests
were weakened and no production bug fix was made here. Self-tuning must not
pick this rejected candidate or mistake a lucky phase for a faster encoder.

[Curated evidence](artifacts/2026-09-26-followup/t593/) contains analysis, exact
per-frame identities, timestamps, damage logs, summaries and probe sources.
Raw streams, binaries and build recipes are retained under
`~/.local/share/blent/profiles/2026-09-26-followup/t593/`, with SHA-256 checksums.
The temporary EVDI output was retired and the normal Android activity restored.
