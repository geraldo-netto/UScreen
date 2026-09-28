# T689 direct input evidence

The four portable contract tests pass. Native Windows runs the common library
suite as ordinary user blentdev (no elevation), including actual owned-window
touch and mouse delivery, held-button drop cleanup and repeated partial creation.
Tests restore cursor position and own their window and synthetic touch device.
Invalid/denied operations, contact transitions, monitor changes, cleanup retry,
replacement and bounded malformed input remain permanent regressions.

Fresh LLVM collections measure all nine shared functions on Linux and all 23
shared/native functions on Windows at >=80% executable-line coverage individually.
Whole-project complexity passes: 6,596 functions, none above 9. GNU cross-check
passes. VM injection is not physical Android/USB or concurrent-device acceptance;
T522 and authenticated integration T673 remain required.

Replay decompressed counters using scripts/coverage/report.py check with the
retained manifest. Scope common/src/direct_input and, on Windows,
common/src/windows/direct_input.rs. Supply native sources.json and prefix
C:\src\blent for Windows; select the corresponding --rust-platform.
See ../../2026-09-28-windows-direct-input.md for mode and integration contracts.
