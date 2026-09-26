# T586: Blent Linux and Android deployment

Both applications are installed and running under the new Blent identity. The
tablet appeared in ADB after the cable check. USB display, touch, app reconnect
and Linux daemon restart are verified; no old settings were migrated.

Native acceptance exposed the pre-existing X11 naming issue T577. Its EDID-based
fix is deployed and verified. Stylus pressure/tilt and late Xorg pen-tool creation
remain T592; camera sharing was not enabled and audio remains unsupported.

## Final installed artifacts and acceptance

The Linux AppImage includes `cc529f1` (T577). SHA-256:
`435e4ac4071314f7bce58c7041938a9c2a1a75cd9f3c8da1f46375787bf6cecd`.
The Android APK remains the signed T585/T587 build, SHA-256:
`4a9c8e578ba45ba2410fd924ea4fde0ea16b8cdd780ff5e22dbb9e1181bb8d80`.
The APK pulled back from the tablet and the installed AppImage both match their
respective build artifacts byte-for-byte. Version: 1.2.3. Linux GUI was reopened
from the updated installation; the service remains active.

The RugKing Pad 2 Pro runs `io.github.geraldo_netto.blent/com.blent.MainActivity`.
Fresh installation was recorded at 04:34 local time. A moving barcode/rectangle
fixture on the 1280x800 virtual output appeared correctly on Android, with
continuing render acknowledgements. App force-stop/relaunch reconnected; the
Linux deployment restart also reconnected and resumed capture. This is logical
reconnection evidence, not a forced cable-unplug or suspend campaign.

Touch/pointer matrices read back `[0.25,0,0.75; 0,0.370370,0; 0,0,1]`, matching
1280x800 at desktop `(3840,0)` within a 5120x2160 desktop. Android tap `(640,400)`
arrived at desktop `(4479,399)` inside the owned test window. Original pointer
position was restored and the temporary pattern window closed. The physical
stylus path is not inferred from this touch result.

During the fixed 30-update/s scene, representative host packet-ready-to-render-ACK
windows reported p50 around 18–20 ms and p95 around 21–25 ms. These are acceptance
observations under concurrent validation load, not optical latency, an A/B
benchmark, allocation measurements or claimed performance improvements.

[Hashes and native evidence](artifacts/2026-09-26-blent-deployment/) include the
owned test-pattern screenshot, mapping/stream logs and bounded memory snapshots.
T589 remains open for instrumented allocation/GC profiling; snapshots cannot
establish per-packet allocation counts or whether further trimming is beneficial.

The sections below retain the initial build/deployment snapshot for provenance.
The original no-tablet blocker and original AppImage hash are historical.

## Initial artifacts

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

## Initial Linux state

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

## Initial automated validation and coverage limits

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
matching codec libraries, but failed with Debian FFmpeg 5.1.9. T588 subsequently corrected the lossy VP9 fixture assumption, retaining all
timing/order assertions and adding exact-pixel coverage; see the follow-up report.

## Initial native acceptance checklist (completed within limits above)

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

Current follow-up results, including T588/T590/T577, are in [validation](2026-09-26-blent-validation.md).
