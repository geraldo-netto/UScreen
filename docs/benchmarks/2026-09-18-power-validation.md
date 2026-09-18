# T388 — partial sustained battery-profile validation

**No battery-saving improvement is established.** Three five-minute measured
phases completed with verified workload visibility. The fourth phase lost
keyboard focus and was rejected automatically; the remaining paired trials and
ten-minute streaming-off control did not run. T388 remains blocked on an
uninterrupted unlocked Linux session with the tablet available for the complete
matrix. The workload was not restarted after focus was lost.

The [power-policy report](2026-09-18-power-policy.md) documents the implemented
opt-in setting and its permanent lifecycle regressions. This report adds physical
USB observations; it does not resolve the outstanding validation requirement.

## Setup and integrity

The planned order was normal static, saver static, saver motion, normal motion,
normal motion, saver motion, saver static, normal static, then streaming off.
Each streaming phase had 30 seconds of warm-up and 300 measured seconds. The
control was planned with 60 seconds of warm-up and 600 measured seconds.

The installed Linux runtime was `52fcd75`; Android sources were unchanged from
`bdaa17a`, including the battery policy. The host remained on standard
`h264_vaapi`, 1280×800, 60 FPS target, quality 18, scale 1, a 4 MiB capture pipe
and stock FFmpeg 6.1.1. The configured 20 Mb/s setting is not an enforced VAAPI
CQP ceiling. Android used its debug APK, app brightness 0.5 and observed display
mode 1, which this tablet reports as 60 Hz. Statistics were hidden. No display,
stream, quality or codec preference changed between profile trials.

The RugKing Pad 2 Pro used its existing 480 Mb/s USB connection. All accepted
battery samples reported USB power and 5 V/500 mA maximum charging values. These
are advertised/reported limits, not measured USB watts. The installed hashes,
USB sysfs identity, display-mode mapping and filtered settings are preserved in
[deployment.json](2026-09-18-power-validation/deployment.json).

The normal T424 guard continuously checked Linux lock status, exact geometry,
focus and overlap. An additional observer checked the tablet's foreground app
and saved preferences about every five seconds, and lock/service state about
every thirty seconds. Stale or failed observations stopped the workload. The
largest tablet-observation gap in the accepted phases was 5.94 seconds; process
samples stayed about five seconds apart. These sampled guards cannot establish
optical presentation or detect every shorter interruption.

A temporary session idle inhibitor prevented automatic desktop locking while
measuring; it did not unlock the session or prevent manual focus changes. It was
released on termination. UScreen, its helper, FFmpeg and the Android app retained
their process identities through the accepted phases. Cinnamon and Xorg also
retained their original identities; no display attachment, service restart or
EVDI module reload was performed for this run.

## Accepted observations

Battery observations within each measured phase span about 270 seconds. Negative
current means whole-device net battery discharge despite USB power. Latencies
below are medians of logged five-second window percentiles, not pooled per-frame
percentiles or optical input-to-photon latency.

| Phase | Net charge change | Net battery current | Encoded FPS | Packet ready → ACK window p50 / p95 |
| --- | ---: | ---: | ---: | ---: |
| Normal static | −9.99 mAh | −133.2 mA | 5.0 | 221.6 / 237.1 ms |
| Battery saver static | −19.98 mAh | −266.4 mA | 5.0 | 221.9 / 237.5 ms |
| Battery saver motion | −39.96 mAh | −532.8 mA | about 60 | 37.3 / 45.4 ms |

The motion FPS median calculated from rounded log intervals is 60.2; it should
not be interpreted as a demonstrated rate above the configured 60 FPS target.
Sparse static output remains near 5 FPS and retains the higher callback latency
seen with this standard encoder profile. The explicit low-latency H.264 option
was not selected for this power comparison.

Normal static held one UScreen partial CPU lock and one Wi-Fi lock. Both saver
phases held neither lock, while the streaming foreground service remained
active. This verifies physical lock ownership, not reduced energy consumption.
Battery temperature was 30.7–31.0 °C, reported thermal status stayed 0, and all
accepted display observations retained 0.5 brightness and the 60-Hz mode.

Median host-pipeline CPU was 0.8%, 1.0% and 16.8% of one core respectively;
Android app CPU was 11.8%, 11.8% and 73.2%. Separate native codec/compositor/system
processes are not included in app CPU. Host one-minute load ranged from 0.29 to
17.31 during accepted phases, with unrelated background work present. The
collector's own ADB polling also contributes an observer effect.

## What the results support

The static charge difference is exactly one 9.99 mAh gauge step. One step over
270 seconds changes the derived rate by 133.2 mA. With one pair, coarse endpoints
and no completed normal-motion pair, these observations establish neither a
saving nor a regression in battery consumption. The opt-in profile removes its
extra locks without demonstrating a battery-life gain on this screen-on USB
workload.

The [older baseline](2026-09-17-device-baseline.md) reported motion window p50
latency of 36.6–37.1 ms and app CPU of 69–73% of one core. The accepted saver-motion
phase is in that neighborhood, not evidence of a broad live-stream improvement.
Its charge rate cannot be attributed to a software regression: the normal-mode
comparison is missing, battery state differs, the warm-up is shorter, and host
background load changed. Historical visibility was also unverified by the later
continuous guard. Isolated codec gains must not be substituted for this missing
matched comparison.

The fourth phase is retained with `invalid.json`. Its summary clears ordinary
performance results and labels calculated observations diagnostic-only. It is
excluded from the table and aggregate analysis. No later phase is represented as
measured.

## Restoration and remaining work

Recorded preferences were restored exactly, including the user's previously
enabled battery saver. UScreen returned to the foreground with its service
active and no extra CPU/Wi-Fi lock. The final process/settings observations are
preserved in [final-health.json](2026-09-18-power-validation/final-health.json)
and the original/final preference files. The temporary startup-investigation
package was removed and experimental replay/control apps were stopped.

To complete T388, repeat the full balanced matrix in an uninterrupted session,
including the sustained streaming-off control and waiting/pen/background/
reconnect observations. The corrected static-control APK passed a short
visibility smoke check with verified workload pixels, but that is not a power
measurement. In that control UScreen would be backgrounded with streaming/service
stopped, not uninstalled or absent from memory.

Only USB ADB was available; both wireless-debugging port properties were empty.
Physical network-route power and an alternative higher-power data source remain
unmeasured. Existing deterministic tests cover network/unknown-route policy and
reconnect hysteresis. They do not measure radio energy. Pen-only validation must
use an isolated setup that avoids exposing the active Cinnamon session to the
unresolved T222 display-attachment problem. The optional low-latency codec should
be measured separately after the matched profile comparison.

## Reproduction and artifacts

The [artifact directory](2026-09-18-power-validation/) contains the full accepted
and rejected observations, analysis, exact collector/orchestration scripts,
control image/source/smoke evidence, deployment hashes, restoration checks,
automated-check logs and `SHA256SUMS`. Scripts retain their original local paths
and serial; use a new directory and the actual tablet/display identifiers when
repeating. `analyze.py.txt` aggregates only complete visibility-verified phases.
Metadata records the checkout at collection time; documentation/tool commits
changed during collection, while the explicitly recorded installed runtime stayed
constant. Do not treat those checkout IDs as the installed binary version.
