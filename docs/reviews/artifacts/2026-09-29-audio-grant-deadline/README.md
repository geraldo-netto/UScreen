# T724: one five-second audio grant deadline

`AudioCapture` previously gave each socket read two seconds, conflicting with
the host adapter's accepted five-second native startup budget. Permanent tests
in `AudioCaptureTest` demonstrate both defects on API 27/34 before the fix:

- A valid grant after 2.2 seconds failed with `SocketTimeoutException`.
- A grant delivered in 32-byte pieces every 1.1 seconds reached capture after
  5.5 seconds, exceeding the complete-grant budget.

`AudioWire.negotiate` now owns one five-second absolute deadline spanning the
64-byte invitation nonce and 92-byte grant. Partial reads cannot renew it. The
deadline clears on success/failure; Stop still closes the socket immediately.
Native grant parsing is separated from deadline ownership. Both permanent tests
pass; the full 669-test Android suite passes. Scoped JaCoCo counters measure
each maintained method in AudioCapture/AudioWire individually above 80%.

Commands were serialized on CPUs 30–31 with nice 19, idle IO and one worker:

```sh
./android/gradlew --no-daemon --max-workers=1 -p android testDebugUnitTest --tests 'com.blent.AudioCaptureTest.t724*'
./android/gradlew --no-daemon --max-workers=1 -p android -I "$PWD/scripts/coverage/android.init.gradle" profileCoverage
python scripts/coverage/report.py snapshot EVIDENCE/manifest.json
python scripts/coverage/report.py check --manifest EVIDENCE/manifest.json --jacoco android/app/build/reports/jacoco/profileCoverage/profileCoverage.xml --scope android/app/src/main/java/com/blent/AudioCapture.kt --scope android/app/src/main/java/com/blent/AudioWire.kt --output EVIDENCE/coverage.json
```

The retained source manifest includes concurrent Rust speaker work; this report
claims only the two named Kotlin files. No physical capture was used.
