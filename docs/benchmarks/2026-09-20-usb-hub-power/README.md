# T538: USB charging-port spot check, 2026-09-20

The tablet remained connected to ADB at USB path `1-1.4` and UScreen continued
receiving render acknowledgements at about 30 FPS. The maintainer clarified that
only the charging cable was moved; the hub model and charging-port wiring remain
unknown.

A battery-only Perfetto trace ran from 17:49:46 to 17:50:51 CEST. The maintainer
reported another charging-port change during the observation, so the result uses
only the final 30 seconds, after that report. The observer changed no application
settings or workload and did not reconnect the display.

| Quantity | Final 30-second observation |
| --- | --- |
| Battery samples | 31, at approximately one-second intervals |
| Mean signed net battery current | -3.93 mA (negative means discharge) |
| Current range | -201 to +151 mA |
| Integrated net charge | -0.0328 mAh |
| Charge-counter change | 0 mAh; coarse gauge cannot resolve this short interval |
| Battery percentage | 97% throughout |
| Android-reported charging maxima | 5 V / 500 mA, unchanged |

This is approximately neutral battery flow during this short live workload.
It does not prove sustained charging, improvement over the previous port, or
actual USB input watts. There is no matched previous-port current trace, and the
live workload was not controlled. A charging status flag alone is insufficient.

Evidence: `metadata.json`, `battery-before.txt`, `battery-after.txt`,
`usb-before.txt`, `power.pbtxt`, `power.pftrace`, `perfetto.log` and `result.json`.
The existing `scripts/benchmarks/rect-power-report.py` validated trace errors,
clock alignment, counter availability and sample coverage, then integrated
signed current over the final 30 seconds. No trace errors/data loss were reported.
T538 remains unresolved pending identified hardware and sustained verification.

## Follow-up with the reported proper USB-C charging plug

At 17:55:14 CEST, Android reported 100% and 9,950,040 µAh, compared with 97%
and 9,680,310 µAh in the earlier observation. These are gauge readings across
uncontrolled use and charging-cable changes, not a measured charging rate or proof
that a particular plug caused the increase. The USB data route was unchanged,
and the UScreen journal contained no session end/error events from 17:51 until
the check during the follow-up. Render ACKs continued at about 30 FPS.

The separate `usb-c-plug-1755/` battery-only observation ran at
17:55:41–17:56:46 CEST. Its 66 samples spanned 64.17 seconds:

| Quantity | Follow-up observation |
| --- | --- |
| Mean signed net battery current | -13.71 mA |
| Current range | -285 to +159 mA |
| Integrated net charge | -0.2444 mAh |
| Charge-counter change | 0 mAh |
| Battery percentage | 100% throughout |
| Android-reported charging maxima | 5 V / 500 mA, unchanged |

The reported battery level has increased since the earlier check. During this
new short sample the current sensor indicates slight net discharge, close to
balance. This does not demonstrate increased charging power or establish a
long-term drain rate. A sample beginning at reported 100% cannot establish the
available charging rate at lower battery levels. The workload was not held
constant across the two observations. No trace errors/data loss were reported;
all raw configuration, trace, counters and battery snapshots are retained in
`usb-c-plug-1755/`. No app settings, workloads or connections were changed by the
observer.

A later post-trace read reported 9,990,000 µAh while remaining at 100%.
`usb-c-plug-1755/battery-postcheck.json` retains a timestamped follow-up snapshot.
This read is outside the integrated trace interval; do not equate its gauge step
with the preceding sample's signed current or infer a charging rate from it.
