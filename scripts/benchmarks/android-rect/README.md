# T419 rectangle rendering research

This is a separate, shell-only `com.blent.rectbench` APK. It combines the
existing production-decoder replay with an experimental RGB888 rectangle
renderer. It neither replaces Blent nor negotiates a new production codec.
The local-fixture comparison excludes host capture, live compression and USB
video delivery. See the [benchmark report](../../../docs/benchmarks/2026-09-19-rectangle-renderer.md)
for measured boundaries and results.

The renderer retains one RGB texture, one full-frame direct decode buffer and
one Zstandard context. Its default reader reuses a direct compressed buffer
sized to the largest validated packet. `--mapped` instead maps the entire
fixture and can therefore inflate process memory considerably. Each decoded
rectangle is uploaded before drawing the complete texture. Empty updates do
not draw. CPU buffer size, metadata, first-frame identity and decompressed
lengths are bounded; this trusted fixture format is **not a network protocol**.

`--verify` reads the complete texture back and compares every RGB byte through
the generator's 64-bit FNV fingerprint. It checks reconstructed GPU texture
content, not optical pixels or a cryptographic wire-integrity property.
Verification runs must be kept separate from performance runs. Ordinary trials
record local admission, read/decode, upload, swap return and supported EGL
display-present timestamps; callback-only H.264 results have a different
boundary. `--presentation` adds an independently sampled SurfaceFlinger ring
for a matched presentation cohort on the current tablet.

## Build

Host requirements: Python 3.12, NumPy, Pillow, a C compiler, the system LZ4 and
Zstandard libraries, stock FFmpeg with the existing VAAPI device, ADB, and the
project's Android SDK/Java environment. The experimental JNI build currently
targets `arm64-v8a` using Android NDK 27.3.13750724; it is not a production
multi-ABI packaging change.

Download upstream archives to a scratch `native` directory as
`lz4-source.tar.gz` and `zstd-source.tar.gz`, then extract their `lib` trees as
`native/lz4/lib` and `native/zstd/lib`. The builder validates both archives
**and every extracted library file** against these pinned versions:

| Library | Source | SHA-256 |
| --- | --- | --- |
| LZ4 1.9.4 | [Upstream archive](https://codeload.github.com/lz4/lz4/tar.gz/refs/tags/v1.9.4) | `0b0e3aa07c8c063ddf40b082bdf7e37a1562bda40a0ff5272957f3e987e0e54b` |
| Zstandard 1.5.5 | [Upstream archive](https://codeload.github.com/facebook/zstd/tar.gz/refs/tags/v1.5.5) | `98e9c3d949d1b924e28e01eccb7deed865eefebf25c2f21c702e5cd5b63b85e1` |

Their upstream license files remain authoritative: LZ4's
[license](https://github.com/lz4/lz4/blob/v1.9.4/LICENSE) and Zstandard's
[BSD license](https://github.com/facebook/zstd/blob/v1.5.5/LICENSE).

From the repository root, supply the real scratch and NDK paths:

```sh
python3 scripts/benchmarks/rect-project.py \
  --directory /tmp/blent-rect-replay \
  --ndk /path/to/android-sdk/ndk/27.3.13750724 \
  --native-sources /path/to/native
```

The build runs permanent JVM fixture-admission tests and writes APK/source
provenance. Existing production decoder sources are copied into the separate
project using `decoder-project.py`; instrumentation stays inside that copy.
The repository's ordinary Python benchmark tests cover clock interpretation,
process identity, source provenance and foreground admission:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_benchmark_rect.py'
```

## Fixtures and device runs

Use a pinned copy of NASA's
[Blue Marble 2002](https://science.nasa.gov/resource/blue-marble-2002/) image for
the photographic-composite control. Record its downloaded hash; the report's
evidence includes the measured image and font identities. The generator uses
the existing text/pen/motion scenes, scrolling text and horizontal photo
movement. The photo case is not natural video footage.

```sh
python3 scripts/benchmarks/rect-fixtures.py \
  --output /tmp/blent-rect-fixtures \
  --font /usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf \
  --photo /path/to/nasa-blue-marble.jpg
python3 scripts/benchmarks/rect-quality.py /tmp/blent-rect-fixtures
adb -s DEVICE_SERIAL install -r /tmp/blent-rect-replay/app/build/outputs/apk/debug/app-debug.apk
python3 scripts/benchmarks/rect-device.py \
  --serial DEVICE_SERIAL --fixtures /tmp/blent-rect-fixtures \
  --provenance /tmp/blent-rect-replay/provenance.json \
  --output /tmp/blent-rect-verification --codecs 1 2 \
  --seconds 2 --warmup 0 --repeats 1 --verify
```

Run the device experiment only while the tablet is available and Blent is
foreground. The replay cancels on touch, focus loss, pause or surface loss;
the driver stops on an incomplete trial and never relaunches another app to
steal focus. No capture display is attached and no Blent preference changes.

Omit `--verify` for timing; defaults are three alternating-order rounds with
20 measured seconds and four warmup seconds per case. Codec IDs are `0` for
hardware H.264, `1` for LZ4 and `2` for Zstandard. Use separate output folders
for controls: `--presentation` for presentation polling, `--mapped` for mmap,
`--scenes text --codecs 0 --text-rate 1` for the matched idle-update control,
and `--keep-process` to exercise repeated Activity teardown without force-stop.
Results preserve APK identity, raw samples and collection boundaries.

For an explicitly available sustained-test window, `rect-power.py` runs five
cases in forward and reverse order: static H.264 at five and one updates/s,
static Zstd rectangles, and pen H.264/Zstd. Its default is eight minutes per
phase (about 83 minutes including transitions). It uses 30-second resource
sampling and on-device Perfetto battery polling every five seconds. The
rectangle trace allocation is bounded to 600 seconds; its storage scales
with the selected rate and duration. Do not combine these allocations with
the earlier fixed-storage short-trial memory results.

```sh
python3 scripts/benchmarks/rect-power.py \
  --serial DEVICE_SERIAL --fixtures /tmp/blent-rect-fixtures \
  --provenance /tmp/blent-rect-replay/provenance.json \
  --output /tmp/blent-rect-power --seconds 480
python3 scripts/benchmarks/rect-power-report.py /tmp/blent-rect-power \
  --processor /path/to/trace_processor_shell
python3 scripts/benchmarks/plot-rect-power.py /tmp/blent-rect-power/summary.json \
  --output /tmp/blent-rect-power-plots
```

Use the new long-duration APK built from current sources. USB remains connected;
no charging, battery simulation or production preference setting is changed.
The report excludes the first 30 measured seconds of each phase and checks
clock conversion, trace errors, counter availability and sampling coverage.
Positive battery current means net charging, not measured USB input power.
Check that all ten phases completed before interpreting the balanced result.
Touch/app switching cancels replay and stops the remaining comparison.
The plotting command requires the complete matrix and writes standalone SVGs
showing both repeats and the raw battery timeline. See the
[presentation and battery follow-up](../../../docs/benchmarks/2026-09-19-presentation-power.md)
for the measured result and its limits.

For presentation diagnosis, capture `android.surfaceflinger.frame` alongside
the existing `--presentation` run. Start tracing before replay and retain its
config, full trace and processor version. `rect-trace-report.py` restricts the
query to the exact replay layer, validates sequence/frame identity against
matched presentation times, rejects trace loss or a truncated measurement
window, and separates queued-but-not-latched frames from unknown omissions.
Use `--processor`, `--trace`, `--trial` and `--output` to provide those files.

```sh
python3 scripts/benchmarks/summarize-rect.py /path/to/completed-results
```

The summarizer reports available completed trials; check the expected matrix
size before calling a run complete. Missing presentations stay missing and
verification runs are rejected as performance evidence. The SurfaceFlinger
comparison uses an interior window to avoid creation/destruction truncating
the statistics ring, validates H.264 sequence identity, and cross-checks RGB
presentation times against EGL. Graphics memory, sampled peaks and app CPU
must not be confused with bounded total device memory or energy consumption.

Negotiated base/generation identity, untrusted packet integrity, live USB
delivery, video fallback and production lifecycle admission remain T419 work.
