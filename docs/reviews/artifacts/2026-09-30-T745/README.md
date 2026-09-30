# T745: isolate GUI persistence fixtures

The September 30 reinstall check found `encoding_gpu="stable-one"` and width2962
in the real user configuration, last written at 14:56:38. Those are T727 fixture
values. `settings_test_app` returned an App using `ConfigStore::default()`; unlike
other persistence tests, the new T727 save test did not replace that store. This
violated the existing T537 rule that GUI tests must not write developer settings.

The permanent `t745_settings_fixture_never_writes_default_user_configuration`
regression runs the actual T727 save test in a child process with isolated
HOME/XDG paths and sentinel default-user settings. Before the fix the child test
passed but changed the sentinel GPU and width; the parent failed. The retained
red log contains only this disposable fixture's values. The subprocess has a
15-second deadline and requires the intended child test to execute and pass.

Every settings fixture now returns an owned temporary directory and an App
whose ConfigStore points inside it. Every caller retains that directory until
the App retires. Existing persistence assertions and explicit-store cases remain.
The regression passes, and all 79 GUI unit tests pass. The live configuration's
hash stayed unchanged throughout verification. Complexity remains at most nine.
Only test code changed: the production prefix of `gui/src/main.rs` is byte-identical
to its parent, and `gui/src/tests/coverage.rs` contains test fixtures only. No
production behavior, coverage exclusion or existing test was weakened.

The earlier host's ordinary shutdown/reload rewrote the active session settings,
clearing the sample GPU/width. Verification confirmed the existing manual
H.264 VAAPI Baseline/RX6600 render path, 1280x800 stream, continuing tablet render
acknowledgements and successful touch/pointer mapping. Other parsed preferences
were unchanged. Original and working post-reload configuration snapshots remain
in the local deployment rollback directory; the polluted snapshot is labelled.

Both host and Android were rebuilt from 741a802, installed and reloaded. Installed
host executable and APK hashes match their release builds; stock libevdi and
autostart remain unchanged. This subsequent test-only correction does not change
those deployed production bytes or claim physical audio/pen acceptance.
