# T611: desktop and Android task flows

The desktop now groups connection status with its display/input Start/Stop
button. Basic video settings remain visible; advanced video and camera controls
use native disclosures. Android has grouped settings, a named 48 dp settings
button, scrollable connection guidance, and explicit camera Start/Stop actions.
Opening settings, expanding a group or applying display preferences does not
start camera capture.

## Audit and prototype before implementation

The [audit](artifacts/2026-09-26-task-batch/t611/audit.md) identified flat control
hierarchy, detached status/actions, inconsistent computer/host terminology,
a small unnamed settings target and no tablet camera Stop action. It relates
these decisions selectively to [Laws of UX](https://lawsofux.com/): Hick’s Law
for fewer initial choices, proximity for grouping, Fitts’s Law for touch targets,
and Jakob’s Law for retaining native controls. These are heuristics, not measured
claims of faster human task completion.

The interactive [task-flow prototype](artifacts/2026-09-26-task-batch/t611/flows.html)
was exercised before production edits. Its
[scripted walkthrough](artifacts/2026-09-26-task-batch/t611/validate-prototype.py)
checks setup/pairing, display start, applying preferences while camera remains
off, camera permission refusal and explicit retry/Stop, advanced controls and
unsupported-backend actions. Checks and screenshot inspection passed at
380×560, 960×360 and 1280×800 without horizontal overflow. Pairing, permissions
and capabilities in this HTML are simulations. Production continues to use
egui and Android Compose; there is no embedded web UI or new runtime dependency.

## Implemented behavior

| Task | Desktop | Android |
|---|---|---|
| Setup and recovery | Display service and tablet status share a card with Start/Stop and contextual connection instructions. Backend capability checks remain authoritative. | Waiting/reconnecting guidance names the computer, explains USB authorization and already-paired network reconnection; content scrolls in short or large-font layouts. |
| Display/input | Native Video, Display & input, Cameras and General tabs retain keyboard/mouse behavior. Encoder, FPS and resolution remain visible. | Display/input, video, camera and app sections have semantic headings. The settings button has an accessible name and 48 dp target. |
| Advanced tuning | Video and camera disclosures retain all existing settings; shared grid wrapping fits the 380 px minimum width. | Video bitrate and app diagnostics use explicit Show/Hide disclosures with expanded/collapsed semantics. |
| Camera consent and stop | Start/Restart applies the current camera profile; Apply only saves it. Camera operations remain separate from display lifecycle. | Start is disabled without a computer request. Stop releases tablet capture; an explicit Start can resume the still-valid request, subject to permission. Neither changes display settings. |

Tablet Stop leaves the computer camera session/output ownership intact; computer
Stop retires that whole session. Initial capture still requires the existing
explicit computer request. Existing foreground/background and permission rules
are unchanged. Unsupported native backends remain visibly unsupported.

## Permanent regressions and validation

Tests were added before changing behavior. The retained
[Android red log](artifacts/2026-09-26-task-batch/t611/android-red.log) shows six
failures for missing accessible target, disclosure and camera actions; the
[desktop red log](artifacts/2026-09-26-task-batch/t611/gui-red.log) shows the advanced
controls exposed before disclosure. Those tests now pass in the normal suites.

- Android `SettingsAccessibilityTest`: descriptive settings action, minimum
  target, advanced visibility and expansion without applying preferences.
- Android `CameraControlsTest`: browsing/Apply leaves capture off; explicit
  requested capture, Stop, denied permission and explicit retry preserve display
  settings and release camera resources.
- Android `SettingsLayoutTest`: short landscape with 1.8× font scale keeps
  connection guidance, settings, advanced bitrate and Apply reachable.
- Desktop T611 tests: disclosure preserves drafts and all four settings tabs
  keep text within 380 px, including expanded controls. Existing T415/T543 and
  lifecycle tests retain their assertions through the new navigation.
- Full Android suite: **564 passed**, zero failures/skips. Release build and lint
  pass. JaCoCo per-method gate: **593/593 at least 80%**.
- Desktop: **71 unit tests plus native accessibility/startup smoke test passed**.
  Linux Rust gate: **1,262/1,262 functions at least 80%**. All changed GUI files
  were recollected; counters for unchanged Rust files come from T625 only after
  exact source-hash comparison. The
  [attestation](artifacts/2026-09-26-task-batch/t611/coverage-attestation.json)
  records this scope. **61 non-Linux functions remain unmeasured**, explicitly
  retained in the all-platform report and existing platform TODO blockers.
- Repository complexity gate: no maintained function above cyclomatic 9.
  Existing malformed-input, bounds and camera lifecycle tests remain in place;
  this change introduces no new wire/parser or native backend contract.

[Evidence directory](artifacts/2026-09-26-task-batch/t611/) contains source
snapshot, coverage counters/reports, build/test logs, prototype and native UI
screenshots. Native Linux GUI screenshots were taken at 440×760 and 380×560 in
an isolated Xvfb session/configuration, without starting display or camera.
The connected RugKing Pad 2 Pro was inspected at 1280×800: waiting screen,
settings and disabled camera actions were visible and readable. No settings
were applied or camera capture started during this inspection.

The signed release APK was verified against the existing package/certificate and
installed with `adb install -r`, preserving app data. Package readback shows
only `io.github.geraldo_netto.blent` among this app's release/profile identities;
Android camera service reports no active clients. The desktop executable was
built from this checkout; building does not replace an installed AppImage.

These checks are developer walkthroughs, automated semantics/layout/lifecycle
regressions and Linux/Android observations. They are not a human usability study,
a TalkBack/desktop screen-reader certification, a Windows/macOS runtime result,
or another native camera/V4L consumer trial. T592 remains skipped for missing
stylus; T621 and other native platform prerequisites remain explicit in TODO.md.

## Before and after screenshots

[Interactive comparison](artifacts/2026-09-26-task-batch/t611/before-after/index.html)
provides desktop, Android settings and Android connection views. These are
actual application captures at matching dimensions, taken after implementation:
the desktop before build was rebuilt from `fa1bfba`, and Android before used the
saved signed pre-redesign APK. After shows `c696f78`. Android was restored to the
current signed release after capture, with app data preserved and camera off.

| View | Before | After |
| --- | --- | --- |
| Desktop, 440×760 | [Screenshot](artifacts/2026-09-26-task-batch/t611/before-after/desktop-before.png) | [Screenshot](artifacts/2026-09-26-task-batch/t611/before-after/desktop-after.png) |
| Android settings, 1280×800 | [Screenshot](artifacts/2026-09-26-task-batch/t611/before-after/android-before-settings.png) | [Screenshot](artifacts/2026-09-26-task-batch/t611/before-after/android-after-settings.png) |
| Android connection, 1280×800 | [Screenshot](artifacts/2026-09-26-task-batch/t611/before-after/android-before-waiting.png) | [Screenshot](artifacts/2026-09-26-task-batch/t611/before-after/android-after-waiting.png) |
