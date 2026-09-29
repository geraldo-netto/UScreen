# T715: installed durable tuning and native startup fixes

Matching release-signed Android and Linux host/GUI/helper builds were installed
on the affected RugKing Pad 2 Pro and Linux host. The user installation now uses
an AppDir with the existing bundled dependencies and newly built binaries; the
previous AppImage remains available for rollback. Signing identity, Android app
data and autostart preference are preserved. This is local deployment, not a
published release or a new distribution-wide portability claim.

## Native defects and permanent regressions

A restart of the initial T714 build selected the saved profile at 10:27:19 CEST,
expired verification at 10:27:25 and reran probes/trials. EVDI did not report ready
until 10:27:32. The valid profile was rejected before the native encoder existed.
The permanent `t715_saved_profile_waits_for_native_encoder_start_without_recalibrating`
test failed at the simulated 13-second delay before the fix. It now covers immediate,
six-second, 13-second and near-deadline startup without running any probes.

`selection/worker.rs::rendered` now distinguishes a bounded 30-second native startup
from the six-second render-verification deadline. Only matching active encoder,
format, decoder and worker evidence starts that deadline. Three fresh ACKs are
still required; replacement generations cannot extend the render deadline. Tests
also retain rejection of missing/wrong/retired evidence, absent ACKs, stale decoder
receipts and previous worker budgets. The existing isolated T497 fixture now
creates a started encoder when exercising absent render progress; its assertions
and deadline remain unchanged. The separate slow libevdi discovery issue T558
remains; skipping calibration does not eliminate hardware startup time.

Native Android UI sampling exposed another completion transition: an exiting
calibration overlay recomposed with “Waiting for your computer…” while fading.
`t715_calibrationCompletionNeverFadesThroughWaitingScreen` reproduced that frame
with Compose's animation clock stopped. The connection overlay now leaves without
an exit fade, exposing the already-ready desktop directly. Its normal connection
loss presentation and calibration timeout remain covered by retained tests.

## Validation and limits

All 66 selection tests pass. All 20 production functions in the changed Rust file
meet the individual 80% executable-line gate. The Android suite passes 622 tests;
all 653 production functions meet the same gate, and lint/release-signature checks
pass. The project complexity check covers 6,887 functions with none above nine.
Rust formatting and whitespace checks pass. Raw counters, source manifests,
failing/passing logs and deployment identities are retained in
[acceptance evidence](artifacts/2026-09-29-display-acceptance/).

The Linux LLVM collection preceded the Android-only presentation edit. A second
manifest records final sources; all Rust production/test source hashes match the
collection manifest. No counters from different Rust revisions were merged.
Android counters were collected after its final edit. No Windows runtime claim is
made: these changes concern the Linux display selector and Android presentation.

First calibration uses actual Android decoder render receipts. Native UI hierarchy
sampling checks progress/desktop presentation without retaining desktop pixels;
its roughly 2.6-second cadence cannot exclude every sub-frame transient. Permanent
Compose transition regressions cover the observed fade defect at individual frames.
An initial non-PTY UI probe produced no hierarchy and is not acceptance evidence;
the retained successful sampler uses a PTY and reads the generated XML.

The native session checks are summarized in the evidence README. Explicit GUI
recalibration and invalid-record recovery supplement the permanent failure tests;
corrupt-record recovery is not a claim that every possible native decoder failure
was injected. Host restart and app reconnect do not establish reboot acceptance.
T715 remains in the ledger for observing profile reuse after the next ordinary
reboot, as explicitly required by its original acceptance condition. No forced
reboot or power-management change was made.
