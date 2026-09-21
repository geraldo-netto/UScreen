# T570 — per-slot shared capture damage

The shared-memory producer now accumulates chroma-aligned dirty spans separately
for each slot. Only an acquired `RAW_WRITING` slot can receive pixels. Conversion
completion clears that slot's history; an encoder-held slot keeps all missed
damage. New generations start fully dirty, and full/incomplete EVDI damage falls
back to a complete conversion. The FIFO and shared adapters use the same bounded
rectangle merge arithmetic. Final-reference release and wire contracts are unchanged.

## Matched local replay

Run `python3 scripts/benchmarks/shared-damage.py OUTPUT`. Baseline `7cac165` uses
full conversion; candidate source hashes and environment are in `metadata.json`.
Five alternating-order pairs, four scales, three workloads, 400 publications per
case, four retained/recycled slots, eight conversion participants, 1280×800 source.
Every published frame contributes to a checksum; all 60 pair checksums matched.
Timing includes fixture mutation, damage bookkeeping, production conversion and
publication through the actual local socket, excluding hash/drain and setup.
No EVDI attachment, encoder, USB, decoder or screen timing is included.

Median of five per-run publication p50 values, microseconds:

| Scale | Workload | Full conversion | Per-slot damage | Reduction |
| --- | --- | --- | --- | --- |
| 1 | Idle repeat | 103.561 | 1.523 | 98.5% |
| 1 | 64×64 patch | 103.500 | 5.731 | 94.5% |
| 1 | Full frame | 238.943 | 220.187 | 7.8% |
| 2 | Idle repeat | 104.492 | 1.102 | 98.9% |
| 2 | 64×64 patch | 106.246 | 4.639 | 95.6% |
| 2 | Full frame | 292.948 | 314.310 | -7.3% |
| 3 | Idle repeat | 128.539 | 0.962 | 99.3% |
| 3 | 64×64 patch | 131.986 | 5.521 | 95.8% |
| 3 | Full frame | 306.765 | 281.987 | 8.1% |
| 4 | Idle repeat | 104.503 | 0.932 | 99.1% |
| 4 | 64×64 patch | 104.382 | 4.208 | 96.0% |
| 4 | Full frame | 248.112 | 205.960 | 17.0% |

Idle and sparse publication improved in every pair at every scale. Full-frame
results vary across pairs; their median differences do not establish a sustained
motion benefit or regression. This optimizes the already selected shared transport;
normal VAAPI/FIFO selection remains unchanged. No end-to-end latency or battery
claim follows from these stage measurements.

## Permanent checks

`host/tests/evdi_helper.rs::t570_shared_slots_retain_damage_and_skip_idle_conversion`
first failed on the old full-conversion path, then passed. Its C fixtures exercise
idle reuse, a slot held across 80 mutations, accumulated chroma damage at scales
1–4, bounded reversed/extreme-coordinate fuzzing, unchanged leased pixels, full-ring
backpressure, full-damage fallback, native grab dispatch, generation replacement
while an old mapping remains leased, and every history-allocation failure.
Existing pixel-oracle and FIFO history tests remain enabled.

All 50 normal C tests passed under the capture coverage runner (new T570 tests use
ASan/UBSan). All 147 maintained capture functions meet 80% executable-line coverage;
all shared damage/ring functions measured 100%. Complexity: no function exceeds 9.
Raw red/green logs, coverage report and test log are retained in `validation.tar.gz`.
