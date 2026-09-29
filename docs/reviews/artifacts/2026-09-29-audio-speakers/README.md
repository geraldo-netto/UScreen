# T719 speaker evidence

## Automated acceptance

- Full Rust workspace libraries/binaries passed: 948 tests, five pre-existing
  ignored cases unchanged. Final native changes additionally pass all seven
  helper unit tests and both private PipeWire integration tests. The complete
  microphone and speaker CLI paths run against authorized fixture peers.
- Full Android suite: 701 tests pass on API 27/34. The final retry used unchanged
  Android sources. An earlier run failed an existing camera retirement assertion;
  its log/XML are retained and the intermittent fixture remains open as T725.
  No camera production code, fixture assertions or deadlines were changed here.
- Per-function executable-line coverage: all 219 scoped Rust functions and 126
  Kotlin methods pass 80%. No new exclusions or weakened thresholds.
- Complexity: 7,279 functions checked, none above nine.
- Actual GUI startup/retirement passes inside private user/mount/network/PID
  namespaces, private `/tmp`, Xvfb and D-Bus. No host input device or X server
  was used. Builds/tests ran serially on CPUs 30–31 at low CPU/IO priority.

`manifest-final.json.gz` and `coverage-final.json.gz` contain the source inventory
and function gate. `native.lcov.gz` comes from a new, empty native LLVM target
after the adapter behavior changes. A final test-only addition exercises an
unconnected native callback through an owned Unix socket pair, with production
code and line positions unchanged. `retained-matching.lcov.gz` keeps only byte-
identical sources from the initial fresh workspace collection; its two changed
native files are excluded. The earlier T718 counters are likewise reused only
for files whose SHA-256 still matches its tracked source manifest. Selection
files and manifests document both imports. `jacoco.xml.gz` is the final Android
collection. No older native line maps were used for changed adapter files.

Permanent red/green regressions cover the missing speaker sink, stale partial
capture blocks after a stall, corrupted/EMPTY SPA metadata and wrapped offsets,
playback-error preservation during teardown, and clean local Stop. Retained red
logs/XML precede the corresponding fixes. New normal-suite tests also exercise
focus denial/loss/regain, independent owners/settings/services, native route and
track replacement, invalid frame/queue/write inputs, stereo ordering, complete
read/write deadlines and owned cleanup.

## Connected tablet

The designated release APK was signed, verified against the tracked certificate
and installed with `adb install -r`, preserving data. `native-identity.json`
identifies the installed APK and executed host/adapter. The tablet was RugKing
Pad 2 Pro, Android API 36; desktop consumers used the normal PulseAudio interface
to the selectable PipeWire sink. No system default or stream route was changed.

The authorized stimulus contained two quiet synthetic tones, 300 ms per channel,
peak 320/32767, plus silence. The sink appeared in 518 ms and Stop completed in
15 ms; removal and unchanged defaults passed. The first probe missed Android's
space-padded UID fields, so that attempt did **not** establish playback counters.
The corrected probe also excludes AudioFlinger's historical retirement rows.

A silent follow-up established the native playback path without repeating the
tones: sink readiness 233 ms, Stop 16 ms, an active 48 kHz stereo track, native
buffer 4,326 frames, advancing server positions `0x4ec0` to `0xe6a0`, and zero
reported underruns. No owned track remained after Stop; the sink was removed and
defaults stayed unchanged. Raw numeric rows and the probe are retained. No PCM,
speech or other audio content was saved. These measurements establish native
delivery/lifecycle, not audible quality, acoustic delay or effective AEC.
Physical duplex, route/USB transitions and acoustic acceptance remain T720;
T718 still retains its separate intentional-speech acceptance requirement.

## Separate findings left open

T725 records the unchanged camera test's intermittent missing rear retirement.
T726 records the user's computer-headset distortion/crackling report. Its bounded
read-only metadata snapshot showed A2DP/SBC stereo, sink 44%, VLC 70%, no capture
or Blent audio node, zero errors in the short graph sample and no recent user
audio-service journal entries. Cause and any Blent relationship remain unproven.
No headset setting was changed. Both findings remain in TODO.md; work stopped
after the speaker implementation as requested.
