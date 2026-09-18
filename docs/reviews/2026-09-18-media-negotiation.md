# T478: bounded decoder/profile negotiation

The version-two control extension is implemented alongside version-one fallback.
It carries per-decoder profile/level/depth and nullable standard feature support,
scoped to the current controller and encoded format revision. Host selection
intersects actual stock encoder probe output with one decoder and the existing
conversion/wire restrictions. Android revalidates the selected implementation
before allocation. See the [protocol description](../video-codecs.md#negotiation)
and [research contract](../media-negotiation.md).

The new inventory and old-host extension regressions were added first and failed
before implementation. The retained normal-suite tests cover API 27/29/30/34,
known/unknown/unsupported profiles and hints, bounds, stale scopes, away-and-back
format changes, named decoder creation, hint omission, same-format decoder
retirement and fallback. The shared JSON fixture is read by Rust and Android.
The original duplicate-settings regression still checks preservation of unrelated
state; actual format changes now deliberately retire capability scope.

Validation: 370 Android tests passed with no failures, errors or skips; lint and
debug APK assembly passed. Rust workspace library/binary suites passed: 344 host
tests with three existing ignored tests, 56 shared-policy tests and 51 GUI tests.
Default workspace/all-target and optional in-process/all-target Clippy passed
with warnings denied. The complexity check found 3656 functions and none above
the maximum of nine. The isolated decoder project builds with the exact current
production dependencies; its three Python preparation/provenance tests pass.

## Physical inventory check

A read-only `app_process` invocation on the connected RugKing Pad 2 Pro queried
the same production inventory implementation at 1280×800/60 FPS, using the
separately built replay APK. The installed UScreen APK was not replaced, no
Activity was launched, and Linux capture/EVDI was not restarted.

The report contains 14 bounded decoder entries. H.264, HEVC and VP9 have hardware
entries; AV1 has software entries. The Unisoc H.264 decoder advertises Baseline,
Constrained Baseline, Main and High, eight-bit, through level 5.1 at this format.
Unisoc HEVC reports Main eight-bit and VP9 reports profile 0 eight-bit, both
through level 5.1. Their reported operating-rate headroom is 120 FPS and their
standard low-latency feature is false. Software AV1 reports the standard feature
as true, with unknown two-times operating-rate support. These are advertisements,
not fresh render, throughput or power measurements; the feature flag alone does
not make AV1 the fastest choice.

The [raw inventory, APK/source provenance, exact sources and test logs](2026-09-18-media-negotiation/)
are retained with SHA-256 checksums. HEVC capability still does not resolve T422's
observed native failure. Rich automatic candidates with missing/unknown probe
metadata are rejected conservatively; existing explicit choices and legacy
fallback remain available. T479 must evaluate measured ranking separately.

Reproduce the read-only inventory without replacing the production app:

```bash
ANDROID_HOME=/path/to/android-sdk python3 scripts/benchmarks/decoder-project.py \
  --directory /tmp/uscreen-inventory --package com.uscreen.decoderbench.candidate
adb push /tmp/uscreen-inventory/app/build/outputs/apk/debug/app-debug.apk \
  /data/local/tmp/uscreen-inventory.apk
adb shell 'CLASSPATH=/data/local/tmp/uscreen-inventory.apk app_process /system/bin com.uscreen.benchmark.NegotiatedInventory 1280 800 60'
```

The HEVC main-tier gate reads the SPS's general tier flag; it does not rewrite
the stream. Its location follows stock FFmpeg's
[profile/tier/level parser](https://github.com/FFmpeg/FFmpeg/blob/n6.1/libavcodec/hevc_ps.c).
Stock `ffprobe` separately verifies codec, profile, dimensions and pixel format.
No FFmpeg patch or new high-tier, HDR, chroma or capture-depth claim is involved.
