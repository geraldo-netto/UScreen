# T603: tablet-guided self-tuning proposal

Research complete. Extend the existing selector and its cache. Do not build a
second optimizer or enable the persistent cache implicitly. The following is a
proposed implementation contract, not shipping behavior.

## Existing behavior verified in code

`host/src/selection/worker/preparation.rs` coordinates probes, cached reuse,
interruption and fallback. `worker/measured.rs` shortlists at most four candidates,
requires measured capacity at requested FPS and quality within 0.5 dB of the
libx264 reference, then compares live render observations within a 36-second
budget. Raw probe ranking also considers capacity, hardware, p95 and startup.
The measured route has its own improvement/quality comparison; these are not a
user-selectable latency/energy objective today.

`worker/cache/context.rs` fingerprints attachment identity/transport, decoder
software/capabilities, stream geometry/FPS/quality, host/helper/FFmpeg binaries,
boot/kernel identity, GPU path, scale, conversion budget, EDID path and pipe
settings. The cached historical winner is live-revalidated; it is not presented
as a freshly ranked optimum. Records are bounded to 64 KiB and one-day age.
The active configuration has `profile_cache=false`. That costs repeat calibration,
not correctness or per-frame throughput. Hit ratio is N/A while disabled.

## Proposed user controls and objectives

Offer an explicit **Optimize for this tablet** action and three objectives.
Keep current behavior until this extension is implemented and validated. A
proposed default for the new action is Balanced; users retain manual settings.

| Objective | Selection rule after validity gates |
|---|---|
| Latency | Lowest repeatable p95, then p99; use CPU/RSS only to break statistically indistinguishable results |
| Efficiency | Lowest measured host CPU cost subject to user FPS, quality and ACK-tail limits; report tablet energy separately when measurable |
| Balanced | Prefer a material CPU saving while keeping p95/p99 within a small explicit margin of the known-good baseline; otherwise keep the faster baseline |

Proposed initial Balanced guardrails for validation: at least 10% lower host CPU,
no delivery regression, and neither ACK percentile worse by more than the larger
of 1 ms or 5%. These are proposed product tradeoffs, not measured universal
thresholds. Reuse the existing 0.5 dB quality floor initially, augmenting it with
text/edge fixtures and worst-frame checks. Do not exchange readability for a
better timing score. CPU seconds are not a battery measurement.

Manual encoder/capacity selections exclude that dimension from optimization.
Requested FPS/scale/quality cannot silently change. If no valid winner has clear
confidence, keep the baseline and show that outcome. No compulsory calibration
on every launch and no camera activation.

## Input and experiment design

1. Snapshot the current attachment lease, negotiated tablet resolution, selected
   stream scale and FPS, decoder receipt and known-good profile. The primary
   workload is this tablet's **1280×800**, not an unrelated 1080p benchmark.
   Use refresh-rate information only when the Android adapter actually reports
   it; distinguish unavailable refresh from requested stream FPS.
2. Probe backend capabilities and host CPU/GPU/driver identity. Include transport
   type, route generation, measured throughput/jitter, charging state and thermal
   state where observable. Missing measurements stay unknown. Do not infer
   sustainable FPS from CPU count or advertised codec names alone.
3. Reuse the existing codec shortlist. Bound thread candidates from T600:
   conversion dispatch already saturates at four participants for full tablet
   frames in the isolated ramp; small dirty regions need fewer. x264 candidates
   should begin at 1/2/4/8, clipped/deduplicated by observed effective threads.
   The current sliced-thread limit of 12 is an upstream geometry heuristic, not
   a universal hardware capacity setting.
4. Change one dimension at a time, then validate the combined winner. Start with
   encoder budget; conversion pool capacity next. Runtime workers belong to a
   separate restart-required experiment: today's Tokio runtime is constructed
   by `#[tokio::main]` and cannot safely be resized by changing an environment
   variable inside a running daemon. Do not add implicit service restarts to
   an ordinary live calibration.
5. Run known content: text/edges, scrolling, motion, sparse updates and idle.
   Interleave baseline/candidate trials with warmup. Match/sweep source phase,
   following T593; a favorable phase is not a codec improvement. Keep capture
   policy fixed. The rejected cadence candidate and unsupported T599
   ownership-only shortcut are excluded; cross-GPU admission still needs T578.

Avoid a Cartesian product of codec × decoder × runtime × encoder × conversion
budgets. Use bounded coordinate trials, prune invalid candidates, then check the
combined result for interactions. This is a local hardware/workload choice,
not proof of the globally lowest possible cost.

## Bounded calibration and acceptance

Use a proposed 90-second interactive budget including warmup and validation.
Cancel promptly on user interaction, detach, resize, route change, profile change,
thermal excursion or explicit Cancel. Restore the known-good profile through
existing epoch/lease ownership. Do not run hidden background stress tests.

Allocate the budget adaptively to the shortlist and repeats; do not pretend a
short trial proves p99 stability. Store exact frame counts, missing/late receipts,
measurement boundaries and between-repeat dispersion. A candidate with too few
samples, contradictory repeats or a confidence interval overlapping the required
improvement keeps the baseline. Offer a separately requested longer calibration
when needed; there is no requirement for a multi-tablet campaign.

Measure separately: capture/encode work, packet-ready-to-render-ACK, host CPU,
live RSS, runnable delay and frame delivery. Compare quality on the same raw
content. Include control traffic and a final reconnect/geometry-change check.
Repeat while thermally stable and record charging/power conditions. Android app
CPU excludes vendor codec services; neither app CPU nor USB charging state alone
establishes tablet energy savings.

## Cache extension and backend boundaries

Version a new record schema for chosen budgets, requested/effective counts,
objective, quality limits, workload identity, confidence and measurement age.
Extend the existing fingerprint with these policy inputs and concrete backend/
driver/route capabilities. Keep conservative invalidation and live revalidation;
never carry a historical winner across an incompatible geometry or attachment.
Retain current opt-in persistence until an explicit product policy changes it.

If enabled later, report eligible lookups, valid hits, misses by reason,
revalidation failures, successful reuse and calibration time avoided. Exclude
cache-disabled sessions from hit-ratio denominators. A high hit ratio with stale
profiles is worse than a safe miss; the existing frame-timing cache is a separate
mechanism and metric.

Portable policy owns experiment order, scoring, cancellation states, bounded
records and fallback choice. Linux adapters own process budgets, EVDI, scheduler
telemetry and service lifecycle. Android owns decoder/display/thermal observations.
Transport adapters own route measurements. UI and wire contracts expose supported
capabilities, not Linux process APIs. Windows stays unvalidated where native
support is absent; macOS remains declined.

Required implementation regressions: manual overrides; candidate deduplication;
no improvement/noisy measurement; quality/delivery rejection; malformed and
oversized records; stale software/geometry/transport/objective keys; disabled
cache; cancelled trials; detach/resize during a trial; old-epoch ACK rejection;
failed combined winner; immediate known-good restoration; no camera side effects.
Test portable policy independently, then validate Linux/Android adapters on the
actual tablet. T603 closes the requested research; no optimizer or cache-default
change is included in this commit.
