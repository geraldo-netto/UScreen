# T495: GUI platform boundary

GUI lifecycle, autostart, privileged setup, executable discovery and platform
identification are routed through `gui/src/platform`. The bounded status sampler
and worker remain shared; Linux and Windows status sources are separate.
Windows reports executable discovery without creating private runtime state or
claiming an attached tablet. URLs use eframe's platform URL handling.

The Windows preview can save shared settings in the roaming known folder and
round-trip Unicode/spaced paths. It labels streaming/input/lifecycle unavailable,
disables Start and autostart, and hides EVDI/uinput setup plus Linux pipe and
conversion controls. Calling an unsupported action directly returns an error.
Saved stream preferences are not evidence of a working Windows encoder.
Linux retains its lifecycle, setup, session, hot-apply and capacity behavior.

Before extraction, MSVC checking failed on Linux imports and Unix-only test
fixtures. Afterward both Windows target checking and the GNU linked GUI pass.
Thirteen Windows unit tests execute under Wine 9, including permanent T495
rendered-label, unsupported-action, Unicode save and executable-suffix checks.
The existing Xvfb/D-Bus integration fixture is correctly Linux-only; its Linux
launch check still passes, alongside all 51 Linux GUI unit tests. A separate
isolated Wine/Xvfb smoke check observed the actual `UScreen` Windows GUI window.
That supports Wine launch feasibility, not native Windows/GPU validation.

Workspace clippy with warnings denied passes for Linux (including optional
in-process encoding) and Windows MSVC. The complexity scan reports 4,054
functions and none above nine. T493 retains native private-state validation;
T497 retains the unverified project-wide per-function coverage/fuzz target.
Nothing was installed or reloaded on the host or tablet.

[Validation logs](2026-09-19-windows-gui/) retain the initial MSVC failure,
Windows tests, Linux tests/launch, Wine window observation and static checks.
