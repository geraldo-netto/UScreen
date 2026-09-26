# Temporary Android profiling

The ordinary Android app keeps the stable package
`io.github.geraldo_netto.blent`. Debug and signed release APKs use that identity;
updates must use the same signing key as the installed app. Do not uninstall the
production app to run a profiling experiment.

`profile` and `optimized` are disposable, shell-only applications. Both use the
profile manifest, with no launcher intent; profiling recipes start activities
explicitly through ADB.
`profile` is debuggable for ART counters; `optimized` inherits release shrinking,
is not debuggable, and uses the local debug key. Neither is a production update.

Build and run an allocation experiment with automatic removal:

```sh
./android/gradlew -p android :app:assembleProfile
python3 scripts/benchmarks/profile-session.py --serial DEVICE \
  --package io.github.geraldo_netto.blent.profile \
  --apk android/app/build/outputs/apk/profile/app-profile.apk -- \
  python3 scripts/benchmarks/android-allocations.py --serial DEVICE --output /tmp/new-allocation-run
```

For camera chunk experiments, use `camera-chunks.py` as the workload. For a full
optimized client, build `:app:assembleOptimized`, use package suffix `.optimized`
and `android/app/build/outputs/apk/optimized/app-optimized.apk`, and pass the
profiling recipe as the workload. Historical T598 recipes require their recorded
measurement environment; the old APK with a launcher is deliberately rejected.

The session runner verifies the APK identity and absence of a launcher before
installation. It refuses an already installed test package, installs without
replacement, waits for the workload, and uninstalls its test package on success,
failure, Ctrl-C or SIGTERM. An interrupted workload is terminated and reaped before
uninstallation. Production package data is never an installation/removal target.
Test results must be copied to the host before the workload exits.

SIGKILL, host power loss or an unreachable tablet can prevent cleanup. Reconnect
and explicitly remove only the named disposable package before retrying:

```sh
adb -s DEVICE uninstall io.github.geraldo_netto.blent.profile
# Only if an optimized profiling session was interrupted:
adb -s DEVICE uninstall io.github.geraldo_netto.blent.optimized
```

Permanent T624 regressions check the merged production/profile/optimized launcher
contracts and temporary-install success, failure, interrupted workload, package
ownership and invalid APK rejection. Development helpers are outside the
production coverage gate; these regressions remain in normal Python discovery.
