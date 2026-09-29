# T723: camera fixture observes native retirement

The API 34 T611 UI case failed with `expected: 2 but was: 1` at its
immediate replacement-open assertion. `CameraBinding.launch` correctly joins
retirement on an IO worker; Compose main-thread idleness is not IO completion.

A permanent latch now holds the fake native close across replacement Start.
Keeping the old immediate assertion reproduced the failure on API 27 and 34
(`forced-red`). The fixture verifies no replacement opens while held, releases
retirement, then waits boundedly for the same open/close assertions. Atomic
counters make observations safe across the worker and test threads. Production
camera behavior is unchanged. The full 665-test Android suite then passed.

Commands (CPU 30–31, nice 19, idle IO, one Gradle worker):

```sh
./android/gradlew --no-daemon --max-workers=1 -p android testDebugUnitTest --tests 'com.blent.CameraControlsTest.t611*'
./android/gradlew --no-daemon --max-workers=1 -p android -I "$PWD/scripts/coverage/android.init.gradle" profileCoverage
```

Compressed original, forced-red and full-green logs are retained here. The
permanent regression remains in `CameraControlsTest.kt`, linked to T723/T611.
