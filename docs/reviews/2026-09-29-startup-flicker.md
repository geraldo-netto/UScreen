# T714: first-connect desktop/connection-screen flicker

The September 29 host journal identifies repeated **automatic live profile
trials** as the source of video reconnections at startup. The maintainer confirms
the tablet returns to its own Blent connection screen between desktop images.
Current Android presentation explains that screen transition: any retired video
connection clears readiness, including deliberate format/decoder changes.

The following investigation describes the pre-fix installation; implementation
and validation are recorded in the final section below. No service restart, device
reconnection, application installation, display change or preference change was
performed. The live installation was unchanged during the investigation.

## Observed sequence

Times below are September 29, 2026, CEST (UTC+02:00). The journal also includes
the host logger's UTC timestamps. [Retained events](artifacts/2026-09-29-startup-flicker/host-events.log)
and [collection facts](artifacts/2026-09-29-startup-flicker/observations.json)
are checksummed beside them.

| Local time | Host action |
| --- | --- |
| 08:50:28.208 | First video connection |
| 08:50:29.407 | Live `libx264` trial; video session ends and reconnects |
| 08:50:34.639 | Codec configuration unavailable after five seconds |
| 08:50:35.408 | Trial advances to `h264_vaapi_baseline` |
| 08:50:37.706 | Trial changes to `h264_vaapi`; video reconnects |
| 08:50:40.737 | Trial changes to `hevc_vaapi`; video reconnects |
| 08:50:43.381 | Baseline H.264 with decoder hints removed; video reconnects |
| 08:50:46.226 | Baseline H.264 with alternate OMX decoder; video reconnects |
| 08:50:49.059 | Final baseline H.264 selection; video reconnects at 49.142 |

Seven selection publications (six trials plus winner), six ended video sessions
and seven video connections occur within roughly 21 seconds. This is one
bounded comparison pass, not evidence of an endless reconnect loop. The initial
libx264 trial spends its six-second window without a successful observation;
the journal does not establish why that trial lacked output. Subsequent
observations identify both decoder-hint and decoder-name changes even when the
encoder name remains baseline H.264. The final trial winner is
`h264_vaapi_baseline` with `c2.unisoc.avc.decoder`, operating-rate hint 120.
Later journal samples show continuing render acknowledgements from that stream.

The clean encoder exits accompanying replacement, with FFmpeg broken-pipe
messages, fit deliberate teardown. These logs do not establish a USB failure
as the cause of the repeated transitions.

## Code path and repeat conditions

1. `host/src/selection/worker.rs::eligible` admits automatic selection after
   geometry and decoder capability discovery. `worker/preparation.rs::prepare`
   performs offline probes, then live comparison for protocol-v2 peers when no
   usable cached profile is selected.
2. `worker/measured.rs::benchmark_with` compares up to four encoder candidates
   and up to two decoder variants. `record_trial` publishes each choice to the
   live stream. `worker.rs::choose` activates and verifies the winner.
3. Android `ControlSession.applyDecoderGreeting` publishes the new stream format;
   `SessionCoordinator.connectFormatCallbacks` forwards it to
   `VideoReceiver.setStreamFormat`. A changed decoder selection, including hints,
   causes `stop()` and replacement startup. Retirement calls `onDisconnected`.
4. `StreamPresentation` immediately posts `connected = false`.
   `BlentUi.ConnectionLayers` shows `ConnectionScreen` whenever display mode has
   `!isConnected`, even while the authenticated control session remains alive.
   The first replacement video frame sets readiness true again. Repeating this
   process produces the reported desktop/connection-screen alternation.

Saved host settings currently select `encoder = "auto"` and
`profile_cache = false`; no `profile-cache.json` exists. Consequently this
configuration does not retain a winner for later session reuse. A fresh
controller/settings epoch can trigger another comparison. This agrees with
[documented automatic selection](../video-codecs.md#automatic-selection).

The existing **Reuse a recent measured profile** option can reduce later live
comparisons, but cannot eliminate the initial comparison or guarantee seamless
startup. It still requires fresh host probes and render verification. Context
changes, reboot, expiry (24 hours) or failed verification cause fresh selection.
Manual encoder selection bypasses the automatic comparison at the cost of
automatic candidate selection. Neither preference was changed during review.

## Fix boundary and required coverage

Represent authenticated calibration/reconfiguration separately from a lost
connection. Keep a stable, truthful presentation during bounded planned
transitions, while still exposing actual disconnects and failures. Do not
silently enable caching, disable calibration or suppress connection loss
indefinitely. A small arbitrary debounce alone cannot represent the observed
six-second unsuccessful trial or the whole calibration pass.

Before behavior changes, add a permanent T714 Android state/UI regression
replaying baseline, high-profile, HEVC, hint-only and decoder-name transitions.
Prove the unwanted full connection-screen returns, then verify the agreed
presentation after the fix. Cover completion, failure/timeout, real video and
control loss, background/foreground, token changes and stale callbacks. If
explicit host calibration state is added to the wire, cover its terminal states,
malformed input and legacy-peer behavior too. Measure each changed maintained
function and retain existing readiness/retirement regressions (T300/T329).

The installed APK is 1.2.3, last updated September 26, and the live host runs an
extracted AppImage; neither is claimed to equal repository HEAD `8b7a5e4`.
Startup Android main-buffer logs had already rolled over when inspected, so
there is no captured tablet callback/screen trace synchronized to these host
events. The host chronology and current code explain the symptom strongly;
physical acceptance must still replay the eventual fix on the affected tablet.
No production code changed during this initial investigation. Implementation
subsequently added regression coverage before changing behavior.

## Maintainer follow-up: durable tuning

The maintainer subsequently specified that calibration should run once to tune
an environment, and subsequent starts should go directly to desktop with that
tuned configuration. This supersedes treating repeated live comparison as
ordinary startup behavior. This became the T714 implementation contract; the observations above remain
historical evidence from the earlier installation.

Recommended contract: retain a durable profile per compatible host/tablet
setup, reuse it across ordinary reconnects and reboots, and verify successful
rendering without rerunning the comparison matrix. Initial calibration gets one
stable progress presentation, exposing the desktop when selection completes.
Offer explicit recalibration and invalidate profiles on relevant compatibility
changes or failed rendering. Exact invalidation inputs and recovery behavior
need specification during implementation; a new boot ID or elapsed 24 hours
alone should not force tuning again. Manual encoder choices remain explicit.

The current opt-in historical cache is not this contract: it is disabled here,
expires at 24 hours and fingerprints boot identity. Its host-probe-before-reuse
path also needs review for direct startup. Persistence/reconnect/reboot and
invalidation regressions must accompany the presentation and recovery coverage
listed above. Safe on-disk validation and current decoder compatibility checks
remain necessary even when an environment has already been tuned.

## Implementation and validation

T714 replaces opt-in, expiring reuse with durable environment-keyed tuning.
Saved records carry the measured profile, survive ordinary retirement/reboot,
and are activated without running the probe matrix. Current decoder support and
fresh matching render receipts still gate activation; failures retain bounded
recovery. The GUI requests recalibration through a persisted generation counter.
Manual encoder choices remain authoritative.

An optional `calibrating` control boolean keeps the Android progress presentation
stable while the live surface continues decoding trials. Successful selection,
fallback/cancellation and legacy/manual paths publish terminal presentation
state. Resetting the control session retires its metadata; repeated updates do
not extend the two-minute presentation deadline.

Before production edits, three Rust T714 regressions failed for profile age,
old opt-out configuration and unnecessary cached-start probes. The Android T714
UI regression failed because the calibration screen did not exist. Permanent
regressions additionally cover reboot context, explicit recalibration, corrupt
records, fresh render verification, normal retirement, terminal states, six
successive video retirements, timeout, malformed metadata and stale sockets.
[Retained validation](artifacts/2026-09-29-startup-flicker/validation/README.md):
620 Android tests pass; all 653 Android functions and 236 scoped Rust functions
pass the individual coverage gate. Linux suites report 912 passing tests
including subprocess fixtures; native Windows rerun reports 351. Complexity,
formatting, Android lint and APK assembly pass. The initial unrelated native
Windows updater fixture timeout remains T716, with original and retry logs.
No updater assertions were changed. Live host/tablet builds are unchanged;
physical display acceptance is tracked separately as T715.

## Subsequent installed acceptance

[T715 native follow-up](2026-09-29-display-acceptance.md) installs matching builds,
fixes the premature pre-encoder verification deadline and the Android overlay exit
frame, and records current validation. The unchanged-installation statements above
describe the original T714 investigation and implementation session.
