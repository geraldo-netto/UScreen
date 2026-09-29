# T718 microphone integration evidence

Permanent normal-suite regressions cover host/Android admission, consent,
authentication, malformed frames, complete-frame deadlines, native readiness,
queue bounds/discontinuities, permissions, foreground-service ownership,
retirement, GUI controls and packaging. Named `*-red.log.gz` files preserve
failures before their fixes; final host, native and full-suite logs retain green
results. T723's separate artifact directory records the camera fixture correction.

Validation ran serially, with `nice -n 19`, idle IO priority, CPUs 30–31, two Cargo
jobs and one Gradle worker. Native PipeWire tests use their own server and no
physical devices. GUI startup ran in private user/mount/network/PID namespaces,
with private `/tmp`, Xvfb, D-Bus, configuration and stubbed external commands.
No live X server, input device or EVDI configuration was changed.

## Automated results

- Rust workspace libraries/binaries: 940 tests passed, five pre-existing ignored
  cases unchanged. Native PipeWire integration and real GUI startup also pass.
- Android: full 665-test Robolectric suite passes on API 27/34.
- Scoped per-function coverage: all 201 Rust functions and 84 Kotlin methods pass
  the 80% gate, including shared configuration/CLI, GUI and host entry points.
- Essential scripts: 61 Python and 66 shell functions pass. Fresh portability
  counters supplement prior counters only where production SHA-256 matches.
- Repository complexity: 7,140 functions, none above nine. The PipeWire stream
  callbacks were extracted without changing behavior; native tests pass after it.
- Signed release APK matches the designated certificate and was installed with
  `adb install -r`, preserving application data.

`coverage-final.json.gz` is the current source manifest; `final-coverage.json.gz`
is the function report. Native inputs are `final-linux.lcov.gz`, the unchanged
source subset `retained-matching.lcov.gz`, and `jacoco.xml.gz`. The prior manifest
and retained-source selection document the reuse. Two obsolete cached executables
still had pre-fix line maps; `stale-objects.json.gz` identifies them. They were
quarantined outside the target directory before final export. No production
function, zero counter or test was excluded to satisfy the gate.

Representative commands (evidence/target paths abbreviated):

```sh
cargo llvm-cov test --no-report --locked --workspace --lib --bins --features blent/native-audio -- --test-threads=1
cargo llvm-cov test --no-report --locked -p blent --features native-audio --bin blent-audio --test audio_native -- --test-threads=1
# Inside the private namespaces described above:
cargo llvm-cov test --no-report --locked -p blent-gui --test startup -- --test-threads=1
./android/gradlew --no-daemon --max-workers=1 -p android -I "$PWD/scripts/coverage/android.init.gradle" profileCoverage
python scripts/coverage/report.py snapshot EVIDENCE/coverage-final.json
cargo llvm-cov report --lcov --output-path EVIDENCE/final-linux.lcov
python scripts/complexity/check.py
```

The report records its exact source scopes. `script-final.py.txt` records the
source-verified incremental script collection. Routine tests remain in the normal
suite; the physical probes below are supporting evidence, not regression tests.

## Bounded physical check

The maintainer previously authorized brief live microphone levels and a test tone
without saved recordings. `native-identity.json` identifies the tablet, PipeWire,
signed APK and binaries; no credentials or device serial are retained.

A PulseAudio-compatible desktop consumer selected **Blent Microphone** explicitly.
The eight-second check received 385,920 samples, 49,817 nonzero; peak 113 and RMS
3.97 (signed 16-bit units). Source publication took 213 ms; explicit Stop removed
it in 15 ms. These are local readiness/retirement measurements, not acoustic
latency or speech-intelligibility measurements. No PCM was saved.

Three further Start/Stop cycles retired in 16/17/21 ms. Hiding the tablet Activity
retired the source in 892 ms; temporarily silencing its microphone through Android
AppOps retired it in 159 ms. The original foreground permission mode and Activity
visibility were restored. Visibility/permission restoration did not restart audio.
Existing default input/output selections were unchanged throughout.

The physical checks preceded the behavior-preserving stream-callback extraction;
the real private PipeWire suite passed against the extracted implementation.
Speech confirmation, physical route/USB changes, duplex drift, acoustic AEC and
end-to-end delay remain explicitly unvalidated. The packet/retirement regressions
cover those transport/lifecycle mechanisms independently of physical acceptance.
