# T586: Blent Linux deployment and prepared Android APK

Linux deployment is complete. Android installation and end-to-end tablet
acceptance remain blocked: neither installed ADB client nor Linux USB
enumeration exposes the reported connected tablet. T586 remains unresolved.

## Artifacts

Built from the T585 rename and T587 allocation change (`77ca590`, `d410952`):

- `dist/blent-1.2.3-x86_64.AppImage`
- `dist/blent-1.2.3-AppImage-sources.tar.gz`, matching bundled dependencies
- `dist/blent-1.2.3-linux-x86_64.tar.gz`
- `android/app/build/outputs/apk/release/app-release.apk`, also staged as
  `dist/blent-1.2.3/blent.apk`

The Android verifier confirms `io.github.geraldo_netto.blent`, launcher
`com.blent.MainActivity`, a non-debuggable release and the designated signing
certificate. The APK SHA-256 is
`4a9c8e578ba45ba2410fd924ea4fde0ea16b8cdd780ff5e22dbb9e1181bb8d80`.
Private signing material was neither changed nor added to Git.

The installed AppImage matches the build artifact byte-for-byte, SHA-256
`9f7b1b0b37a6e1a0a0ea3dc9828cd2f9136a85c12bcb73a5368cacd98e2c780c`.
Source and packaged MIT license hashes match the original. The ABI check
reports glibc requirements of 2.34 for daemon/helper, 2.35 for GUI and 2.33 for
libevdi; this is not validation of every Linux distribution or GPU backend.

## Installed Linux state

The old user service was stopped and disabled. Its service, autostart/menu
entries and command symlinks were moved to
`~/.local/share/blent/deployment-backup-20260926/` for rollback. Old private
settings and signing-key custody remain untouched and are not read as Blent
settings. No compatibility aliases or preference migration were installed.

The normal AppImage user installer registered
`~/.local/share/blent/appimage/Blent.AppImage`, `~/.local/bin/blent`,
`~/.local/bin/blent-gui`, the desktop entry and `blent.service`. The new service
was explicitly started; autostart remains a fresh Blent preference.
`blent --version` reports 1.2.3, daemon status reports running, and the native
GUI process remains running without a startup panic. A native service restart
also passed, retiring the prior owner and establishing a new active owner.

Doctor confirms two EVDI devices, writable uinput, bundled capture helper,
FFmpeg, ADB and ffprobe. KScreen is absent on this non-KDE desktop. No tablet
capture helper is running because there is no attached ADB device. Startup
alone does not validate display, pen, camera, audio or reconnect behavior.

## Automated validation and coverage limits

- Default Linux workspace: 805 passed, three existing ignored tests.
- Optional in-process encoder workspace: 689 passed, two existing ignored tests.
- Android: 520 passed, none skipped; release build and lint passed. All 559
  measured production functions met 80% executable-line coverage.
- C capture: all 153 functions met 80%.
- Essential scripts: all 61 Python and 60 shell functions met 80%. Collection
  resumed after installing a missing Java runtime for the complexity tests;
  prior passing tooling/Python suites and their counters were retained.
- Complexity: 5,609 functions checked, none above nine. Rust formatting passed.

The combined Rust per-function gate does **not** pass: its Linux report lists
1,212 of 1,224 functions passing, ten below threshold and two unmeasured.
The two unmeasured functions are foreign-platform scheduling adapters wrongly
included in Linux scope; they are not native validation evidence. T590 tracks
that reporter correction and remaining command/scheduling/logging/lifecycle
coverage. T572 retains the two camera coverage gaps (75% and 28.57%). EDID
generation modified for Blent has 100% measured coverage. The separate report
retains another 59 Windows-only functions and the existing native prerequisites.
No full-project coverage pass is claimed.

The T421 VP9/AV1 picture-content fixture passed with bundled FFmpeg 6.1.6 and
matching codec libraries, but failed with Debian FFmpeg 5.1.9. T588 records the
required baseline/range investigation; no regression was skipped or weakened.

## Remaining native acceptance

Once the target tablet is visible and authorized in ADB, install the verified
release APK, stop the old UScreen app and explicitly launch
`io.github.geraldo_netto.blent/com.blent.MainActivity`. Start with fresh settings;
do not uninstall unrelated apps or copy old private data. Verify discovery,
display/input, enabled camera/audio paths, restart and reconnect. T589 separately
tracks current native allocation/GC/latency profiling before further memory
policy changes. No device performance or battery improvement is claimed.

Artifact hashes and local deployment observations are retained in
[deployment evidence](artifacts/2026-09-26-blent-deployment/).

## T591 coverage attribution correction

The original collection above is historical. Reprocessing its unchanged source
snapshot and raw counters after fixing `cfg` interpretation gives 1,222 Linux
functions: 1,212 pass, ten below 80%, none unmeasured. All 61 non-Linux functions
remain visible and unmeasured in the full report. T590/T572 remain open; native
Windows/macOS obligations remain T493/T497/T583.

The old prefix expression missed `windows` after a feature predicate and every
macOS-only guard. Permanent T591 regressions failed before the fix and pass after
it. They cover reordered/nested guards, boolean alternatives, unknown features,
module descendants, and rejection of foreign counters. Existing shared-module
ownership regressions remain. All 58 coverage-tool tests pass; complexity audit
measures 5,615 functions with none above nine. No application behavior changed.
