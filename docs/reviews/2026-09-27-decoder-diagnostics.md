# T417: read-only decoder diagnostics

Android's **App & diagnostics** section now shows the owning active decoder's
name, stream dimensions/FPS, hardware status, advertised standard low-latency
support and advertised twice-frame-rate headroom. It separately lists the
performance keys actually supplied to MediaCodec and the watchdog's decision
to disable hints after repeated stalls. Effective settings remain explicitly
unknown. Advertised support is not measured throughput or a latency guarantee;
missing standard low-latency support is not proof of slow decoding.

Optional native metadata queries run on the setup worker, outside the owner/UI
lock. Failed queries remain unknown and cannot reject an otherwise compatible
decoder. API27 keeps unavailable hardware/standard-low-latency information
unknown. Invalid dimension/rate ranges and malformed optional hint values are
handled without inventing support or overflowing the headroom calculation.

A draft snapshot belongs to each startup attempt. Publication follows successful
codec start and current-owner checks; abandoned or late startup cannot publish.
Release clears the active snapshot while retaining watchdog fallback history.
The UI consumes a read-only StateFlow, including codec/format replacement and
retirement while Settings remains open. No profile, preference, decoder default,
network contract or stream-setting transition is changed.

The maintainer confirmed on 2026-09-30 that read-only diagnostics complete T417.
The item is closed; additional profile-selection controls are outside this accepted
feature. Existing T386 measurements do not justify changing decoder defaults.

## Validation boundary

Update 2026-09-28: [T701](2026-09-28-compose-test-lifecycle.md) corrected the
shared-worker lifecycle and the current full instrumented Android suite passes.
The collection limits below describe the original T417 run and remain historical
evidence; the decoder UI scope decision and physical measurement limits remain.

Normal-suite tests cover supported/unsupported/unknown capability values,
API27/API34, missing native metadata, malformed hint values, invalid/range-boundary
inputs, exact configured requests, codec/stream changes, stopped/late startup,
watchdog fallback and UI updates. API27's Robolectric codec shadow lacks native
identity queries; the probe fixture explicitly supplies these while using real
framework capability objects. These are JVM/Robolectric fixtures, not a new
physical-tablet decoder performance campaign.

The first full instrumented run repeatedly timed out attaching Compose roots in
`SettingsAccessibilityTest`; a narrower combined-worker run reproduced it.
Thread sampling found `ComposeIdlingResource.isBusyAttaching`. Both runs were
stopped after repeated failures, so neither is a full-suite pass. The same tests
and the new UI fixture passed together in a fresh focused worker. T701 retains
investigation of preceding-test/environment state; no runtime cause is claimed.

A collection-only Gradle init script then used one test class per process.
All **304 selected tests passed**, with no skipped or weakened assertions. Native
JaCoCo counters pass **all 165 production functions/callback methods** in the
six changed Kotlin files, individually at least 80%, with no missing counters.
The normal build/test configuration is unchanged; this scoped isolated result
does not close T701 or establish that the full combined suite passes.

The selected classes include decoder, Settings, session, Activity UI, display,
receiver, decoded-output, Surface callback, token-delivery, regression and stream
format contracts. `scripts/complexity/check.py` reports 6,423 functions, none above
nine. Rust formatting and repository whitespace checks pass.

[Evidence](artifacts/2026-09-27-decoder-diagnostics/) retains the source snapshot,
raw JaCoCo execution/XML data, per-function report, test/collection logs, temporary
isolation configuration, failed combined-run logs and sampled thread stacks.
No application was installed or live sharing session restarted for this work.
