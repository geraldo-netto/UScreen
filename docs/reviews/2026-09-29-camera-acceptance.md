# T621: native webcam consumer acceptance

The authorized September 29 session passes Linux V4L2 and real Chrome consumer
acceptance on the connected RugKing Pad 2 Pro (Android API 36). Both applications
use the matching installation recorded under [T715](2026-09-29-display-acceptance.md).
The signed APK was read back and matched the build byte-for-byte; host, GUI and
capture helper hashes also matched. Camera code was unchanged during this
acceptance; the installed display fixes do not change camera ownership.

Two existing exclusive-capability v4l2loopback outputs, `/dev/video20` (Blent Front)
and `/dev/video21` (Blent Rear), were exercised at 1280x720, 30 FPS, with the default
3000 kbit/s target and 150 ms freshness budget. The production `blent cameras`
command uses the same native pipeline as the GUI. An isolated headless Chrome
session on localhost used real `getUserMedia`, exact device selection and
`audio:false`. The automatic-permission UI flag did not supply a fake camera.

| Stage | Front mean RGB | Rear mean RGB | Consumer state |
| --- | ---: | ---: | --- |
| Front active | 87.25 | 0.00 | Both live |
| Front stopped | 0.00 | 0.00 | Both ended after black output |
| Rear active | 0.00 | 139.91 | Both live |
| Rear stopped | 0.00 | 0.00 | Both ended after black output |
| Front after rapid replacements | 85.71 | 0.00 | Both live |
| Final stop | 0.00 | 0.00 | Both ended after black output |

Each observation summarizes ten 64x36 in-memory canvas samples. Active samples
had nonzero pixels; every inactive/stopped sample was exactly black. These values
establish delivery and blank retirement, not color fidelity or presentation latency.
No camera image/video was saved. A stopped producer ends the browser track; a later
Start was opened through a fresh explicit device selection.

Between stable runs, four production sessions alternated front/rear, each stopped
about 600 ms after launch. A fresh front session then captured successfully, with
its inactive counterpart still black. Final Android `dumpsys media.camera` reported
`Active Camera Clients: []`; only the display's 8890/8891 reverse mappings remained.
The display service stayed running throughout camera acceptance. These native
replacements support T706/T707 lifecycle acceptance. Deliberately delayed and stale
Camera2 callback ordering remains covered by the permanent automated retirement
regressions; this experiment does not claim to force every hardware callback race.

[Evidence](artifacts/2026-09-29-camera-acceptance/) retains numeric consumer results,
the bounded acceptance harness/page, source/build identity reference and command
outcome. All 622 Android tests passed on the installed source revision, including
retained camera consent, permission, freshness, transport, callback and retirement
tests. All 653 Android production functions meet individual 80% coverage. Prior
native decoder/stall evidence remains in [T614–T618](2026-09-26-camera-freshness.md).
No camera implementation was changed, so no artificial new behavioral test was added.

Physical cable removal is separate T540 and is not established by these software
Start/Stop checks. No meeting was joined, audio captured, lock forced or broad
battery/performance campaign performed. T621 can be removed independently; audio
implementation still requires completion of T540.
