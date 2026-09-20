# T536 — desktop login without the graphical session target

The Cinnamon/X11 session on 2026-09-20 had a loaded, enabled but inactive
`uscreen.service`; `graphical-session.target` was inactive. The GUI selected
systemd from the unit's `LoadState`, and the source installer selected it from
manager reachability. Both removed the XDG entry, leaving no login trigger.
AppImage registration also removed the entry when the service was enabled.

The GUI and installers now share `scripts/uscreen-service-autostart.desktop`,
which starts the existing user service at desktop login. The graphical target
and the entry address the same service, so systemd retains one daemon.
Disabling through the GUI disables the unit and removes the entry. Source
reinstallation and AppImage registration repair the entry for an enabled
service without enabling a previously disabled preference or starting a daemon
during installation. The existing direct route remains for systems without a
usable user service.

## Permanent regressions

Before changing production code, these normal-suite regressions failed:

| Test | Reproduced failure |
| --- | --- |
| `common/tests/autostart.rs::t536_managed_autostart_works_without_graphical_target_and_disables_cleanly` | Enabling the service left no desktop entry while the graphical target stayed inactive. |
| `scripts/tests/test_autostart.py::test_t536_installer_enable_and_reinstall_preserve_cinnamon_login` | Both explicit enable cases and preserving an already enabled service left no desktop entry. |
| `scripts/tests/test_appimage.py::test_t536_registration_preserves_managed_login_without_graphical_target` | Registration lost the enabled service's desktop launch. |

The same regressions pass after the fix. They execute the generated entries
through GIO with an isolated systemctl fixture, check repeated launches address
one service, retain disabled preferences, and verify GUI disable removes both
login routes. The fixture never activates the graphical target or contacts the
real manager. `test_t536_failed_autostart_staging_preserves_existing_login_entry`
also checks that a failed write retains the previous entry and removes the
temporary file. Existing direct-launch quoting, unavailable-manager, AppImage
ownership, GUI lifecycle and installer regressions remain in the normal suites.

## Validation scope

The complete `uscreen-config` suite and GUI suite passed, including the isolated
GUI startup test. LLVM coverage for all ten functions in
`common/src/linux/autostart.rs` passed the 80% per-function gate: nine measured
100%; `remove_desktop_entry` measured 83.33%. The whole-project complexity check
found no function above nine. Desktop-entry validation, shell syntax and Rust
format checks passed.

The complete essential-script coverage run passed for 48 Python and 57 shell
functions, including the normal tooling, script, complexity and coverage suites.
The host lacked RPM tools for the retained packaging regressions; those tools
were extracted into a temporary directory, without installing system packages.

Local deployment uses the AppImage, as requested by the maintainer. The host,
GUI and helper were rebuilt in the existing Debian 12 AppImage build container;
the bundle passed the glibc 2.36 ceiling check. The AppImage includes its matching
dependency sources and notices. Native-package fixtures remain automated test
coverage, not the local installation route.

The built image passed a network-disabled, clean Debian 12 container check
without installing runtime packages: all bundled executable dependencies
resolved, bundled FFmpeg encoded H.264, ADB ran, and extract-and-run user
registration plus daemon status worked. The packaged GUI also opened under an
isolated Xvfb display in the build container. Local AppImage registration
preserved the enabled service and replaced its executable path with the stable
installed image launcher; the installed image and login entry matched the
built artifact and shared template. No daemon was started during registration.

This validates login dispatch and persistence with isolated service state. It
does not claim a real logout/reboot trial, EVDI attachment validation, or a fix
for the separate T222 Xorg crash.
