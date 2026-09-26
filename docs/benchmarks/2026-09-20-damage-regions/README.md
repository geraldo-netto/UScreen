# Changed-region conversion — 2026-09-20

Current status (2026-09-26): horizontal-span conversion is implemented and active
in the deployed Blent helper. See [shared capture activation](../2026-09-21-shared-capture/README.md)
and [current deployment](../../reviews/2026-09-26-blent-deployment.md).
T222 is deferred and does not block ordinary restarts. Measurements and deployment
steps below describe the original September 20 experiment.

T554 narrows BGRA-to-NV12 conversion to the horizontal area covered by EVDI
rectangles, rounded outward to complete scaled 2×2 chroma blocks. Previously,
any damage in a row caused conversion across its full width. Clean rows and
idle frames were already skipped; this change does not introduce those savings.

Each of the three NV12 buffers keeps its own row mask and horizontal spans.
Damage accumulates in all histories, including buffers not used for several
frames. Metadata follows pixels through pointer swaps; a writer's leased pixels
remain immutable. One interval per chroma row bounds memory and bookkeeping;
separated rectangles on that row include the gap between them. At 1280×800,
the three span arrays add 9,600 bytes. The existing Auto/manual conversion-thread
capacity is preserved; dispatch estimates source work using the span widths.

The math uses plain coordinates and pixel types. EVDI rectangles enter through
the Linux capture adapter; no Android, UI, configuration or wire change is
needed. FFmpeg still consumes full NV12 frames and produces H.264. This does not
send raw changed tiles across USB or change Android rendering. T418/T419 remain
separate transport work.

## Isolated measurements

Host: Ryzen 9 7945HX, GCC 13.3.0; exact compiler, kernel, baseline revision and
measured source hashes are in [metadata.json](metadata.json). Two header
comments were clarified afterward; executable behavior is unchanged. One synthetic 1280×800
source, eight-participant conversion pool, scales 1–4. Five paired trials per
case/scale, 128 measured updates per process; baseline/candidate order alternates
by trial. Six full-buffer rotations precede measurement. The second measurement
pass below ran after the coverage suite finished. The desktop remained active;
this is not a dedicated, frequency-locked benchmark machine.

Baseline is `d1de64a`'s row-only modules. Candidate uses the same math with spans.
The fixture changes source pixels, marks damage, converts and rotates buffers;
full output checksums match for all 100 pairs. At scale 3 the output dimensions
crop to even values, as production does.

Values below are the median of five trial p50 conversion times, in microseconds.
CPU is the median of trial mean process CPU per iteration; it includes synthetic
source mutation, damage marking, conversion/worker work and publication/claim.
It excludes setup, worker creation and the final checksum. Conversion timing
includes dispatch and joining. These are not pooled percentiles, encode/USB/
render latency, tablet power measurements or a Bluetooth audio improvement.

| Scale | Changed source area | Row conversion µs | Span conversion µs | Row CPU µs | Span CPU µs |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | 64×64 patch | 26.29 | 3.78 | 26.98 | 4.20 |
| 1 | 8×800 strip | 119.13 | 9.73 | 435.86 | 12.45 |
| 1 | 640×360 window | 106.20 | 82.36 | 232.03 | 97.57 |
| 1 | 1280×800 full frame | 132.07 | 127.87 | 636.22 | 554.24 |
| 1 | Two separated 16×64 patches | 26.24 | 25.80 | 27.29 | 26.69 |
| 2 | 64×64 patch | 52.60 | 2.98 | 53.00 | 3.33 |
| 2 | 8×800 strip | 177.24 | 5.38 | 685.60 | 7.55 |
| 2 | 640×360 window | 157.65 | 147.22 | 346.26 | 161.72 |
| 2 | 1280×800 full frame | 174.47 | 188.67 | 767.95 | 829.28 |
| 2 | Two separated 16×64 patches | 53.25 | 53.02 | 54.21 | 54.11 |
| 3 | 64×64 patch | 69.27 | 4.22 | 70.99 | 4.55 |
| 3 | 8×800 strip | 211.13 | 8.02 | 798.88 | 10.29 |
| 3 | 640×360 window | 187.57 | 177.02 | 390.56 | 189.75 |
| 3 | 1280×800 full frame | 215.79 | 212.76 | 943.57 | 927.53 |
| 3 | Two separated 16×64 patches | 70.05 | 70.30 | 70.74 | 72.50 |
| 4 | 64×64 patch | 43.07 | 2.71 | 44.21 | 3.03 |
| 4 | 8×800 strip | 132.26 | 7.50 | 511.47 | 9.63 |
| 4 | 640×360 window | 116.88 | 115.26 | 256.61 | 126.87 |
| 4 | 1280×800 full frame | 140.09 | 144.53 | 628.25 | 701.61 |
| 4 | Two separated 16×64 patches | 42.90 | 44.13 | 43.56 | 44.93 |

Native 64×64 patch conversion improved about 7× (26.29 to 3.78 µs). Narrow
vertical changes also avoid waking workers for otherwise almost-full-width
work. Window updates use less CPU; fewer workers mean wall time does not fall
in proportion to converted area. Full-frame conversion has no pixel savings:
its median trial p50 changed by approximately -3.2%, +8.1%, -1.4%, +3.2% at
scales 1–4. Trial variation and extra metadata work prevent a universal
full-frame speedup claim. Damage marking itself increases from roughly
0.04–0.12 µs to 0.06–1.02 µs in these runs. Separated patches spanning both edges
show the conservative interval's limit: most of the intervening row is still
converted. Keep these costs alongside the sparse-update gains.

[raw.json](raw.json) retains each trial's conversion p50/p95, damage p50, process
CPU, active jobs and checksum. This bounded local replay does not reopen T382's
declined multi-tablet/large-machine campaign. Reproduce from repository root:

```sh
python3 scripts/benchmarks/damage_regions.py /tmp/blent-region-replay
```

The output directory must be new. `--baseline` selects a pre-region revision;
`--trials` permits 1–9 paired rounds. The script compiles only isolated math and
buffer modules and never opens EVDI, an encoder or an Android connection.

## Correctness and delivery

The permanent normal-suite regression
`t554_capture_converts_only_chroma_blocks_intersecting_damage` first failed
against the original code because luma outside horizontal damage was overwritten
([red](regression-red.log)); it passes with spans ([green](regression-green.log)).
It exercises the actual EVDI rectangle adapter and native/scaled conversion,
including odd source dimensions and preserved luma/chroma sentinels.

`t554_regions_clip_fuzzed_bounds_preserve_pixels_and_follow_buffer_leases`
covers 2,048 deterministic fuzz rectangles plus explicit extreme, reversed,
empty, off-frame and invalid-scale cases. Its independent full-frame oracle
checks byte-identical NV12 after repeated partial updates, disjoint rectangle
merges, dropped publications, held writer leases, retirement/resize and full
refresh. Additional native/scaled cases cover single/eight-participant pools,
empty spans, source padding and destination guard bytes. Both sanitizer and
optimized builds run in the normal conversion suite ([result](regions-tests.log)).
Allocation failure tests retain their original cases and now cover all three
new span allocations too.

The normal conversion/helper/module/retirement suites pass under the capture
coverage gate ([test log](capture-tests.log)): **114/114 production C functions
meet 80% executable-line coverage**, with no missing counters
([report](capture-coverage.json)). Later added scaled-worker tests pass separately;
production sources did not change after the coverage snapshot. The whole-project
complexity gate reports **5,079 functions, none above 9** ([log](complexity.log)).
The region and full-oracle equivalence suites also pass with the Debian 12
release compiler ([log](debian12-tests.log)). These are simulated
capture/lifecycle tests, not live EVDI or native Windows validation. Existing platform blockers stay in TODO.md.

The production helper also compiles at `-O3 -Wall -Wextra -Werror` against the
installed bundled libevdi, and a help-only invocation prints usage (the private helper returns status 1). The system
has no development `-levdi` linker name, so that isolated link used the existing
bundled library explicitly. No daemon, helper, encoder, display mode or ADB connection was restarted during
these measurements. At that time the running display used the previous helper, and T556 tracked
activation. T222 was then treated as a restart blocker; that historical
disposition was superseded by the accepted deferral and later activation. Android needs no
update for this host-only optimization.


After the replay, the helper was rebuilt with Debian 12 and refreshed inside
an AppImage using the previously validated application/dependency bundle. ABI
and dependency checks pass; the AppImage's CLI help smoke test exits successfully.
The new image is installed at the existing user launcher path, with the old
image retained as a rollback copy. [deployment.json](deployment.json) records
both hashes and paths. Readback confirms that the live daemon, helper and
encoder still use the same executable hashes as before installation. The conversion was not active in the September 20 display session. No service
restart or Android installation was made during that experiment; subsequent
activation and deployment are linked above.
