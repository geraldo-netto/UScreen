# Physical network power comparison protocol

T487 prepares a comparison of USB and physical-network display transport on the
existing Linux host and tablet. This is a measurement plan, not a power-saving
result. Complete the matched USB Battery saver comparison and streaming-off
control in T388 first. T738 retains the physical measurements until an identified
network arrangement and an undisturbed host/tablet window are available.

## Required arrangement

Record the access point/router model, host connection (wired or wireless), tablet
Wi-Fi band/channel, approximate distance, competing traffic, ADB connection route
and USB power/data wiring. Use the same tablet and application versions. Confirm
the selected ADB endpoint actually transports the stream over the network; a
reachable network endpoint alone does not establish which connection Blent chose.

Keep power delivery the same in both transport conditions, with the tablet below
full charge. The USB cable may remain connected for power during network trials,
but select the network endpoint explicitly and verify the active transport.
Record any condition that cannot hold charging/data roles constant; such trials
cannot isolate transport power. Do not combine this with the alternative-supply
comparison in T683/T739 or change Wi-Fi debugging settings without recording and
restoring them. An unknown transport is a separate correctness case, never a
measurement substitute for a known USB or network route.

## Shared trial method

1. Record host/APK/helper hashes, signing identity, configuration, selected and
   effective encoder/decoder, GPU/driver, transport, brightness, refresh rate,
   battery level/temperature and application preferences. Use the T388 matched
   workload: 1280×800/60 FPS target, H.264 Constrained Baseline/QP18, brightness
   50% and display refresh 60 Hz. If current validated software changes a baseline,
   establish a new matched USB control rather than mixing historical results.
2. Keep camera and audio sharing off, and retain the same observer schedule and
   static/motion scene definitions in every trial. Confirm actual foreground,
   keyguard, process and presentation state; do not infer them from an open app.
3. For each static and motion workload, compare USB/network and normal/Battery
   saver in balanced forward/reverse order. Use two repetitions, 30-second warm-ups
   and five-minute measurement windows. Sixteen trials require about 88 minutes;
   a ten-minute streaming-off control plus setup makes this roughly a two-hour
   reserved window. Stop rather than accepting disturbed or incomplete pairs.
4. Capture raw monotonic/UTC boundaries, battery charge/current/voltage/temperature,
   host and Android CPU with measured tick/page-size units, frame cadence, software
   render acknowledgments, session identity and actual CPU/Wi-Fi lock ownership.
   Existing `scripts/benchmarks/run-baseline.py`, `observe.py`,
   `android_session.py` and visibility/integrity reporters supply those observation
   primitives. Configure a current, bounded run from them; the archived T388
   orchestration contains historical package/geometry constants and is not a
   current unattended launch command.
5. Reject a phase on focus/keyguard changes, unexpected restarts/reconnects,
   transport or settings changes, missing observations, trace loss, thermal
   changes invalidating a pair, or unrelated workload intrusion. Preserve rejected
   raw evidence separately. Restore all preferences, selected endpoint and
   debugging state in cleanup and verify restoration even after a failed trial.

## Correctness checks before measurement

Exercise waiting, foreground/background, Stop and reconnect on both routes.
Observe real locks alongside the policy: Battery saver removes extra locks when
inactive; while active, known USB can omit the Wi-Fi lock, whereas network and
unknown routes retain conservative Wi-Fi handling (`StreamingPower.kt`). Do not
weaken unknown-route behavior to improve a power result. Any reproducible defect
needs a permanent failing automated regression before correction.

## Reporting and acceptance

Retain one manifest per phase and compare complete matched pairs. Report signed
net battery current and integrated net charge separately from gauge change and
advertised USB maxima. Charge counters that do not move at their resolution are
inconclusive; advertised volts/amps are not measured input watts. Report CPU,
cadence, reconnect/lock correctness and software latency boundaries alongside
power. Software render acknowledgments do not establish optical latency.

Publish the exact arrangement, accepted/rejected windows, raw data, paired
differences and limitations. Do not claim a gain from an unpaired run or expand
this into the declined multi-tablet/large-machine campaign. The earlier
[partial USB comparison](2026-09-18-usb-power-comparison.md) remains historical.
