# T673 authenticated touch/mouse preview

The Windows USB/Wi-Fi session pipeline now composes the existing native monitor
and injection adapters with a negotiated, independent finger-mouse protocol.
Host preferences select the exact monitor, initial mode and allowed devices;
Android supplies Touch/Mouse, Right-click and Drag next gesture controls.
Mode selection persists through the same configuration store used by the daemon.
Controller replacement, attachment loss, mode changes and shutdown retire owned
contacts/buttons. Invalid events, stale monitor snapshots and injection failures
reject input and retire the session. Stylus remains unavailable.

## Permanent regressions

- `host/src/input/direct/tests.rs` covers negotiation/version and JSON bounds,
  disabled modes, failed injection, topology changes, legacy touch translation,
  unsupported pen requests, owner replacement and watched lifecycle changes.
- `host/src/input/direct/windows/tests.rs` exercises the native worker through
  authenticated sockets, an owned Windows window, touch/mouse delivery, mode
  replacement and saved-mode restoration.
- `host/tests/windows_direct_usb.rs` exercises the daemon's actual USB monitor,
  native ADB fixture, session composition, negotiated owned-window delivery,
  owner replacement, token rotation, disconnect/reconnect and native touch/button
  cleanup. It uses a private injected configuration store and no physical tablet.
- `android/app/src/test/java/com/blent/DirectInputTest.kt` covers API 27/34
  gestures, invalid coordinates/tools, explicit controls, negotiation and reset.
  The two added behavioral regressions failed before their fixes: a rejected
  negotiation send could reauthenticate a retired socket; a missing or
  reclassified tracked finger could leave a drag held. `handshake-red.log.gz`
  and `pointer-red.log.gz` retain those failures. Both tests remain in the normal
  suite and pass unchanged after their fixes.
- Shared configuration and GUI tests cover monitor bounds, independent device
  permissions, selection, persistence and opt-in defaults. Existing T689 native
  assertions remain; their owned-window helper is shared in `testdata/`.

## Results and limits

Native Windows 11 x64, Enterprise Evaluation build 26200, runs the full workspace
suite as interactive `blentdev` with Task Scheduler RunLevel 0 (unelevated):
395 reported passes, zero failures. Linux runs all workspace library/binary tests,
diagnostics, portable session/USB/host regressions and the actual GUI startup:
900 passes plus one isolated fixture subprocess pass. The existing two Windows
and five Linux opt-in benchmark exclusions are unchanged; no regression was
removed, skipped or weakened.

Android: 614 tests pass without skips; lint and debug APK assembly pass.
Every one of 649 Android functions/methods meets the 80% executable-line threshold.
The Rust scope covers the changed production files listed in `rust-scopes.json`:
174/174 Linux functions pass, 202/202 functions pass with combined Linux/native
Windows counters, and an independent native Windows gate passes all 29 functions
in the direct-input worker, Windows factory, preview composition and shared input
settings UI. Shared functions use combined native counters; these are scoped
results, not a new whole-repository coverage claim. Whole-project complexity:
6,854 functions, none above 9. Rust formatting and whitespace checks pass.

The optional GNU cross-build could not start because
`x86_64-w64-mingw32-dlltool` is absent on this host. The Windows evidence is actual
native MSVC compilation and execution, not a cross-build or Wine result.
An additional native `cargo check --locked --workspace --all-features` fails
with the pre-existing `MediaBytes::new` feature-gating error at
`host/src/annex_b.rs:90`, now tracked separately as T713. That caller and its
constructor gates are unchanged by T673. The passing native suite and coverage
above use the default feature set; no all-features success is claimed.
The same E0599 was reproduced with `cargo check --locked -p blent --lib
--all-features` from an isolated native export of baseline `0cab073`, including
its original fixture assets. Both failure logs are retained here as
`windows-all-features-failure.log.gz` and
`windows-baseline-all-features-failure.log.gz`.
Physical tablet delivery, elevated targets, lock/unlock, sleep/resume and
concurrent physical mouse behavior remain T522. Display streaming and stylus
support are not enabled. No physical device, display layout or driver was changed.

## Reproduction and provenance

Sources were frozen before each final Rust measurement and measured in fresh
LLVM target directories. `windows-sources.json.gz` contains native SHA-256
fingerprints; `manifest.json.gz` is the final source/function inventory. Android
was measured after its final production change; its collection snapshot is also
retained. `provenance.json` verifies that subsequent Rust-test additions changed
no Android source before the final manifest. Native test logs were decoded from
PowerShell UTF-16; source fingerprints were decoded from UTF-8 with BOM without
changing any fingerprint value. Raw counters were not filtered.

Linux collection:

```sh
CARGO_LLVM_COV_TARGET_DIR=/fresh/linux-target cargo llvm-cov --locked \
  --workspace --lib --bins --test diagnostics --test startup \
  --test portable_session --test portable_usb --test config_diagnostics \
  --test portable_host --lcov --output-path linux.lcov
```

Native Windows collection uses `cargo llvm-cov --locked --workspace
--remap-path-prefix --lcov --output-path windows.lcov` in a fresh target directory.
Android uses `./android/gradlew -p android -I "$PWD/scripts/coverage/android.init.gradle"
profileCoverage lintDebug assembleDebug`.

Replay the decompressed evidence using `scripts/coverage/report.py check` with
the retained manifest, both LCOV files, native fingerprints, both
`--rust-platform linux --rust-platform windows` flags and each scope from
`rust-scopes.json`. The Android gate uses `--jacoco android.xml --scope
android/app/src/main/`. The independent native direct-input gate uses only
Windows counters/fingerprints with scopes `host/src/input/direct`,
`host/src/usb/preview.rs` and `gui/src/direct_input_settings.rs`.
