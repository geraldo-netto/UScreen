# T540: physical USB camera acceptance

Status: **passed** on September 29, 2026. The operator physically unplugged and
reconnected the tablet during active front-camera sharing, then confirmed desktop
recovery: “it takes a while but it recovers”. T540 is complete; T621 and T540 no
longer block shared audio implementation T717. Audio remains unimplemented.

The [operator-assisted harness](artifacts/2026-09-29-usb-camera-readiness/accept.py)
passed with exit code 0 on the same installed builds as
[T621](2026-09-29-camera-acceptance.md). It used isolated localhost Chrome, real
V4L2 capture and numeric-only canvas samples. No camera image or audio was saved.
The test used Front at 1280x720 with both named outputs open in the consumer.

| Observation (CEST) | Result |
| --- | --- |
| 10:56:22, before removal | Front nonblack; inactive Rear black |
| 10:57:15, ADB disappears | Physical disconnect observed |
| 10:57:20, disconnected outputs | Front and Rear exactly black in every sample |
| 10:57:29, ADB returns | Physical reconnect observed |
| 10:57:32–33, before fresh Start | No Android camera clients; Front remains black; only display mappings 8890/8891 |
| 10:57:39, explicit fresh Start | Front nonblack again; Rear black |
| 10:57:41, final Stop | Front black; camera clients empty; only display mappings remain |

The camera did not resume automatically. A fresh explicit Start and consumer
selection restored frames. Device-side camera state cannot be queried while USB
is absent; the first post-reconnect query, before fresh Start, confirms release.
The harness retained black output and resource checks independently of display
service state. Camera requests and browser tracks were retired after the check.

Display recovery was established separately by the operator, post-reconnect render
ACKs and the tablet UI: a desktop SurfaceView with its gear control, without a
connection/calibration message. The display service remained active. Host events
show USB detection at 08:57:30.994340 UTC, saved-profile selection at 08:57:31.008390,
helper readiness at 08:57:43.399527 and frame production at 08:57:43.477642. The
profile reused libx264 with one worker and c2.unisoc.avc.decoder at operating rate
120; no new comparative calibration was logged.

Helper readiness took **12.405 seconds after host USB detection**. This is a host
startup boundary, not an optical first-desktop measurement. It is consistent with
the stock-libevdi acquisition delay already tracked as T558; this run did not
isolate its internal scan time. The initial video connection retried while waiting
for capture. The 08:57:48.525201 fresh latency window contains 245 render ACK samples
(p50 16.1 ms, p95 18.1 ms packet-ready to ACK); the earlier 08:57:43 report may
include pre-disconnect samples and is not used to identify the first fresh frame.
T558's dependency-policy decision remains unresolved. T715 still requires an
ordinary reboot observation; cable recovery is not a reboot substitute.

[Retained evidence](artifacts/2026-09-29-usb-camera-readiness/) includes original
numeric observations, timestamped host events, operator confirmation, the bounded
harness and hashes. Existing permanent permission, disconnect, black-output and
retirement regressions remain unchanged. This acceptance changes no production
behavior and needs no artificial new unit regression.
