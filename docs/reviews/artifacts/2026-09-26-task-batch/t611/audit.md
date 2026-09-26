# T611 audit and prototype, before production UI changes

Current code: `gui/src/main.rs`, `gui/src/camera_settings.rs`, Android
`BlentUi.kt`, `SettingsSheet.kt`, `CameraControls.kt`, `CameraBinding.kt`.

Findings within T611's requested redesign:

- Host shows many equal-weight video controls before the user needs advanced
  tuning. Keep encoder, frame rate and resolution visible; group quality,
  bitrate/depth/scaling, calibration and capacity under an explicit disclosure.
- Status uses implementation language (“Daemon”) and separates status from its
  primary Start/Stop context. Lead with connection/readiness, setup recovery and
  display action; keep diagnostics subordinate without inventing streaming proof.
- Android settings is a long undivided column. Group display/input, video,
  camera and app preferences with matching names and clear save/apply scope.
- Android's top-right settings target is 38 dp and lacks a descriptive semantic
  label. Give it a 48 dp accessible button, retaining normal touch/focus behavior.
- Camera settings say to use the computer but provide no local stop action.
  Expose Stop camera while active and explicit restart of the existing host
  invitation. A missing invitation disables Start; opening settings/Apply never
  requests capture. Host Start still owns initial route/output setup.
- Connection instructions assume USB and say “host/PC”; use “computer” in
  user instructions and explain that an already paired network route reconnects
  automatically. Do not imply bonding or native Windows runtime support.

Reference selectively applied: [Laws of UX](https://lawsofux.com/). Hick’s Law
motivates fewer initial choices; proximity motivates grouped status/actions;
Fitts’s Law motivates the larger settings/Stop targets; Jakob’s Law motivates
native desktop keyboard controls and Android Material switches/sheets. These
are design heuristics, not measured claims about user performance.

`flows.html` is an interactive task-flow prototype, not a shipped UI or backend.
Before implementation, scripted Chrome checks passed at desktop 380×560 and
Android 960×360 / 1280×800: pair, start display, apply without camera activation,
permission refusal, explicit camera Start/Stop, advanced controls, unsupported
backend labels and disabled actions. No horizontal overflow; content scrolls.
Screenshots were inspected. This is a developer walkthrough and layout check,
not a human usability study, assistive-technology certification or native device
acceptance. Production retains each platform's widgets and navigation rather
than embedding this HTML.

Implementation validation must retain existing UI/lifecycle regressions and add
permanent tests for accessible settings size/name, progressive disclosure,
camera-off when browsing/applying, local stop preserving display, and narrow /
large-font reachability. The pre-existing backend permission/lifecycle and
unsupported-platform contracts remain authoritative.
