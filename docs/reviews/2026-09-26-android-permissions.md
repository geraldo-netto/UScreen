# T619 — runtime permissions during ADB installation

`make android-install` now executes `adb install -r -g`; the doctor hint and
current installation guides use the same flags. Android grants the runtime
permissions declared by this APK (currently Camera). Normal permissions are
handled by the OS; signature/special permissions and USB authorization are not
promised by this flag. Manual APK installs retain Android's permission prompts.
No permission grant starts the camera or bypasses Start/Stop/lifecycle checks.

The permanent normal-suite regression in `scripts/tests/test_make.py` executes
the actual Make rule against a recording ADB fixture. It failed before the fix
because `-g` was absent, then passed; it also checks there is only an install
invocation, with no launch or camera broadcast. All seven Make tests and 41
host doctor tests pass. Documentation/hint changes need no artificial tests.

The existing signed production APK was granted Camera permission with `pm grant`
and verified through package-manager state. `dumpsys media.camera` reported an
empty active-client list afterward. The retained [evidence](artifacts/2026-09-26-android-permissions/)
contains no credentials or camera pictures.

The final host daemon/GUI bundle is installed with the updated doctor hint.
Three post-startup observations retained the same x264 PID, one encoder worker
and 1280×800 geometry. Config bytes remained unchanged, the Linux pointer was
visible, and camera clients stayed empty. [Deployment identity](artifacts/2026-09-26-android-permissions/deployment.json).
