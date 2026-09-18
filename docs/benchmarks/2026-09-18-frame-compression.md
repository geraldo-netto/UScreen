# T419 — lossless RGB frame compression screening

**Changed RGB rectangles with Zstandard level 1 are worth a presentation
prototype; keep the existing H.264 path while that is measured.** This research
does not establish an Android battery or capture-to-display improvement.
The remaining Android reconstruction, GPU upload, presentation, recovery and
power work stays open as T419.

The [tablet resource follow-up](2026-09-18-frame-compression-resources.md) records
current app/codec/compositor CPU and memory observations and defines the next
controlled experiment, including buffer reuse, allocation peaks and memory
traffic. Its live-session observation does not compare compression candidates.
The subsequent [Android renderer comparison](2026-09-19-rectangle-renderer.md)
measures both compressors against hardware H.264 with local fixtures: CPU
improves on some UI scenes, while app memory and photo/scroll bandwidth rise.
It does not establish an end-to-end latency or battery advantage.

## Scope and comparable inputs

The host is the existing Ryzen 9 7945HX workstation. Inputs are the original
1280×800 RGB text, pen and moving-rectangle generator used by T399/T400.
Regenerating and converting all three 240-frame sequences produced the exact
archived NV12 SHA-256 hashes; the font was also verified. Lossless candidates
preserve RGB before chroma subsampling. H.264's NV12 reference and QP 18 output
have already lost information: this is a size/cost comparison, not equal
perceptual quality or equivalent pixel formats.

The screen includes text at 5 and 60 FPS, pen and motion at 60 FPS, and a
text→pen→motion→text transition. Each nominal sequence lasts four seconds.
Every delta method refreshes the complete RGB image once per second. Cases are:

- Whole-frame LZ4 and Zstandard level 1.
- One changed bounding rectangle, changed 32×32 tiles, and full-frame XOR
  against the preceding reconstructed frame, each with LZ4 and Zstandard.
- Whole-frame PNG at compression level 1 as an image-codec control. QOI was
  not measured in this first screen.

Three rounds reverse codec/method order in the second round. Every decoded
payload and reconstructed RGB frame is checked exactly. The original matrix
has 135 trials; three rectangle-preparation variants add 90 trials. Together
they verify **44,100 complete RGB reconstructions**. Source generation and
verification are outside timing; buffer allocation, packing, compression,
decompression and reconstruction are included in the host timings.

Python 3, NumPy 1.26.4, Pillow 10.2.0, LZ4 1.9.4 and Zstandard 1.5.5 were used.
No CPU affinity/frequency lock or isolated desktop was imposed. Results are
medians of three trial percentiles at `floor(p × (N − 1))`, including full
refreshes. Sparse text has only 20 pictures per trial, so tails are coarse.

## Payload size and host processing

Rates include compressed payloads and estimated rectangle/tile coordinates,
but exclude an unspecified production transport envelope. They are not measured
USB wire rates. The H.264 values are the preserved stock VAAPI Constrained
Baseline/CAVLC streams from the [T479 comparison](2026-09-18-profile-selection.md),
including that run's actual keyframes and pacing.

| Workload | H.264 baseline Mb/s | Full RGB Zstd Mb/s | RGB rectangles Zstd Mb/s | RGB tiles Zstd Mb/s | RGB XOR Zstd Mb/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| Text, 5 FPS | 0.688 | 2.986 | 0.597 | 0.597 | 0.601 |
| Pen, 60 FPS | 0.952 | 37.742 | 0.676 | 0.693 | 0.762 |
| Synthetic motion, 60 FPS | 7.115 | 36.807 | 1.428 | 5.921 | 2.040 |

Full-frame PNG costs about 18–21 ms at the host median in this implementation
and about 57.6 Mb/s for motion. Full-frame LZ4 motion uses 53.9 Mb/s. Neither
offers a reason to replace the measured H.264 path in this screen.

The original NumPy rectangle detector materializes coordinates for every
changed pixel, while a cropped RGB array is noncontiguous. Reducing rows/columns
over a two-dimensional **byte** view, then making the crop contiguous before
serializing it, avoids expensive small-element iteration. All three variants
retain exactly the same payload sizes and reconstructed pixels.
The original rectangle motion median was 12.35 ms; the final prototype's median
is 0.74 ms. This is an optimization of the research harness, not a production
UScreen speedup or proof that every rectangle implementation beats tiles.

| Final RGB rectangle/Zstd prototype | Host preparation + compression + decompression + reconstruction p50 / p95 / p99, ms |
| --- | ---: |
| Text, 5 FPS | 0.310 / 2.987 / 2.987 |
| Pen, 60 FPS | 0.322 / 0.444 / 1.542 |
| Synthetic motion, 60 FPS | 0.741 / 1.917 / 2.339 |
| Text/pen/motion transition, 60 FPS | 0.322 / 1.173 / 1.734 |

These synthetic scenes have large flat-colour regions. A separate four-frame,
seeded independent RGB-noise control requires **1,475–1,482 Mb/s at a projected
60 FPS** for every tested LZ4/Zstd representation. That is a byte-count
projection, not a transmitted stream. It exceeds the current USB connection's
measured capacity. Real photo/video/scrolling content and bounded fallback
therefore matter; the low synthetic-motion rate is not a universal video gain.

## Native decompression on the actual tablet

A temporary, statically linked AArch64 command-line executable ran through ADB
on the RugKing Pad 2 Pro while UScreen remained foreground. It used unmodified
LZ4 1.9.4 and Zstandard 1.5.5, GCC 12 cross-compilation and ordinary native
decompression APIs. It is a Linux/static-library microbenchmark on Android,
not an Android NDK/JNI or presentation implementation. The executable and
fixtures were removed from the tablet afterward.

Twenty-four fixture sets select original frame numbers
`0, 1, 30, 59, 60, 119, 180, 239`; empty rectangle/tile updates have no sample.
Each nonempty sample is decompressed and its complete output fingerprint checked
before timing 50 repeated calls. Every timed call checks the output length.
Three rounds reverse file order in the middle round: **25,800 timed calls**.
Output buffers are reused and warmed. Checksums, file reads, application/JNI
work, RGB reconstruction, texture upload and drawing are outside timing.

| Zstd sample set | Tablet decompression p50 / p95, ms |
| --- | ---: |
| Full text RGB | 1.743 / 1.833 |
| Pen rectangles, including refresh samples | 0.002 / 1.932 |
| Motion rectangles, including refresh samples | 0.453 / 1.873 |
| Motion tiles, including refresh samples | 0.727 / 1.844 |
| Motion XOR, including refresh samples | 1.350 / 1.873 |

Sample selection is deliberately not a cadence-weighted frame distribution.
These percentiles cannot be added to host or H.264 percentiles to manufacture
an end-to-end result. The 43-second native run observed unchanged 49% and
4,865,130 µAh readings at 30.9°C. That short, coarse observation is **not** an
energy comparison or evidence of zero current draw.

## Secondary compression and local transport

Recompressing the preserved H.264 access units independently reduced motion
bytes by only 0.4–0.6% in the first recorded phase, text by roughly 3–5%, and
pen by 8–10%. These are scene-specific size observations before adding a new
compression envelope or Android decoding stage. They do not establish a latency
or power benefit; leave encoded-video recompression disabled.

Raw compression inside the Linux helper→encoder path is a separate question.
These RGB payload measurements are not a FIFO or AVFrame IPC benchmark. Compare
that proposal with T389's direct filling and T418's leased shared-memory design;
do not insert compression/decompression ahead of stock FFmpeg on these numbers.
Nothing here patches FFmpeg or changes installed host/tablet behaviour.

## Remaining acceptance work and reproduction

T419 now has a measured shortlist. Next, preserve EVDI damage rectangles where
useful and prototype bounded Android reconstruction and GPU presentation with
explicit generation/base-frame identity, atomic updates, full-refresh recovery
and a video fallback. Use broader photo/video/scrolling inputs. Compare actual
end-to-end latency and sustained power against hardware H.264 on this tablet.
Add permanent corruption, stale-base, reconnect and lifecycle regressions before
production admission. No multi-tablet or large-machine campaign is required.

The [evidence directory](2026-09-18-frame-compression/) retains per-frame results,
native CSVs, fixture bytes, generator verification, exact scripts, native source,
compiler/input hashes, all intermediate detector variants and checksums.
`summarize.py.txt` recomputes `analysis.json` after extracting `raw-results.tar.gz`
and `native-fixtures.tar.gz` into the same directory. The source archive includes
the commands and dependency versions needed to repeat the experiments.

Primary interfaces: [LZ4 bounded decompression](https://github.com/lz4/lz4/blob/v1.9.4/lib/lz4.h),
[Zstandard APIs](https://github.com/facebook/zstd/blob/v1.5.5/lib/zstd.h), and
[QOI](https://qoiformat.org/) for the unmeasured alternative image control.
