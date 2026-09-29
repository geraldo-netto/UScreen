# T721: host mouse failure after concurrent validation

The maintainer reports that the physical host mouse stopped moving and clicking,
while keyboard shortcuts still worked. An ordinary reboot restored normal mouse
operation. The cause is unconfirmed; this is incident evidence, not a fix or a
passing regression result.

## Timeline

Times below are September 29, 2026, CEST (UTC+02:00).

- 12:49:28: the previous session launched Rust workspace coverage, Android
  coverage, and script coverage concurrently. Script coverage also invokes the
  Rust tooling suite and packaging tests (`scripts/coverage/script_check.py`).
- 12:54:21: the maintainer first reported an unresponsive desktop.
- 12:54:31: the process sample included active RPM packaging and gzip. This
  establishes concurrent work, not a cause of the later mouse failure.
- 12:54:59: the previous session restricted 13 owned test processes to CPUs
  30–31, nice 19, idle I/O priority. Symptoms persisted afterward.
- 12:59:04: the maintainer reported continued sluggishness. Later samples in
  that session reported 97–98% CPU idle and no swap activity; test jobs had ended.
- 12:59:59: the maintainer identified the physical-monitor mouse as very slow.
  Read-only queries then showed the Logitech mouse enabled, an identity
  transformation matrix, acceleration speed 0, and the physical monitor at
  3840×2160/60 Hz. These snapshots do not measure delivered mouse events.
- 13:03:19: the previous boot's journal ends. The new boot's first entry is
  13:03:32. The exact onset of complete movement/click loss was not captured.
- 13:03:38: Xorg PID1907 aborted during the new boot with the T700 startup
  signature. This happened after the reported mouse failure and cannot be its
  preceding trigger.
- After reboot: the maintainer confirmed normal mouse movement/clicks and that
  keyboard shortcuts had remained usable during failure. The installed Blent
  host and its capture helper were running again during investigation.

## Retained evidence and limits

- [Selected pre-reboot observations](artifacts/2026-09-29-t721-pointer/pre-reboot-observations.txt)
  are extracted from prior session tool results. Their timestamps are UTC.
  They are snapshots, not continuous event or latency measurements.
- [Kernel query](artifacts/2026-09-29-t721-pointer/pre-reboot-kernel-query.txt)
  found no matching USB/HID/input/AMDGPU/OOM/stall/watchdog messages between
  12:45 and 13:03 in boot `335820b37dc24d03beaed72c660aeb15`.
  Absence of a log message does not establish healthy input delivery.
- [Cinnamon session excerpt](artifacts/2026-09-29-t721-pointer/pre-reboot-session-excerpt.txt)
  contains earlier actor warnings and shutdown-time X connection loss. Neither
  identifies a mouse grab, missing hardware reports, or the responsible client.
- [Post-reboot Xorg excerpt](artifacts/2026-09-29-t721-pointer/post-reboot-xorg.txt)
  records `dixRegisterPrivateKey`, `glamor_init`, `AddGPUScreen`, and signal 6.
  `/var/log/Xorg.0.log.old` now belongs to this new boot, not the incident boot;
  the incident's Xorg file was overwritten during server rotation.
- Root-only `/var/log/lightdm/x-0.log` may retain earlier Xorg stderr, but it
  could not be read: `sudo -n` returned `sudo: a password is required`.
  No elevated read or system change was performed.

The reviewed post-report commands only queried mouse settings; they did not
change acceleration or disable the receiver. The tested uinput constructor
path uses a checked process-local syscall fixture
(`host/src/input/linux/coverage_tests.rs`). The benchmark visibility fixture
creates a private Xvfb; its concurrent connection failures remain T722.
These checks narrow the investigation but do not establish that every executed
test was isolated or that Blent did not contribute to the incident.

## Remaining diagnosis and required regression

The following bounded observation is prepared for a recurrence on this Linux
X11 host. It uses installed `evtest`, `xinput`, and `timeout`; it does not grab
devices, inject events, or change preferences. Device IDs can change on reboot.
Before running either reader, verify the physical mouse identity:

```sh
POINTER_DEVICE=/dev/input/by-id/usb-Logitech_USB_Receiver-if01-event-mouse
POINTER_ID=$(xinput list --id-only 'Logitech USB Receiver Mouse')
readlink -f "$POINTER_DEVICE"
udevadm info --query=property --name="$POINTER_DEVICE" | rg '^ID_INPUT|^DEVNAME'
xinput list --short "$POINTER_ID"
xinput list-props "$POINTER_ID" | rg 'Device Node|Device Enabled|Transformation|Accel Speed'
test -r "$POINTER_DEVICE"
```

Proceed only with exactly one slave pointer whose `Device Node` matches the
resolved path, `ID_INPUT_MOUSE=1`, and no `ID_INPUT_KEYBOARD=1`. Do not select a
master pointer, keyboard, similarly named consumer-control device, or touchpad.
An unreadable node is missing diagnostic access; do not change its permissions
or the user's groups as part of capture.

In separate terminals, run these two 15-second readers while making ordinary
mouse movements. Both print only event categories/counts, never coordinates or
key values. The timeout status 124 is expected when each observation completes;
other failures must be retained, not interpreted as zero events.

```sh
set -o pipefail
timeout --kill-after=1s 15s stdbuf -oL evtest "$POINTER_DEVICE" |
  awk '/^Event: time/ { count[$6]++; if (!first) first=$3; last=$3 }
       END { print "kernel_first=" first, "kernel_last=" last;
             for (kind in count) print kind, count[kind] }'
printf 'kernel_reader_status=%s\n' "$?"
```

```sh
set -o pipefail
timeout --kill-after=1s 15s stdbuf -oL xinput test "$POINTER_ID" |
  awk '$1 == "motion" { motion++ }
       $1 == "button" && $2 == "press" { press++ }
       $1 == "button" && $2 == "release" { release++ }
       END { print "xinput_motion=" motion+0, "press=" press+0, "release=" release+0 }'
printf 'xinput_reader_status=%s\n' "$?"
```

Set the verified variables in each terminal. Capture these bounded state queries
during the same failure, with a timestamp and whether keyboard shortcuts and
ordinary window repaint still work:

```sh
date --iso-8601=seconds
timeout --kill-after=1s 2s xinput query-state "$POINTER_ID"
timeout --kill-after=1s 2s xinput list --short
timeout --kill-after=1s 2s xinput query-state 'Blent Pointer'
timeout --kill-after=1s 2s xinput query-state 'Blent Touch'
```

Physical kernel events without XInput events narrow the fault to delivery above
the receiver. Both streams continuing with an immobile/unusable pointer require
investigation of grabs, virtual input state, and the compositor. Neither pattern
alone proves a Blent fault. A fast X query does not prove that mouse events reach
ordinary clients or get rendered. Healthy-state observations cannot substitute
for failure-time evidence, and the original incident remains unreproduced.

After capture, isolate the input owner and test recovery in the smallest scope
supported by that evidence. Do not restart the display manager or unload GPU/
EVDI drivers as a speculative diagnostic on the active desktop. Broad concurrent
validation stays idle for this investigation; any later validation should start
with one bounded workload and constrained CPU/I/O use.

No causal reproduction or automated regression exists yet. Once a faulty
transition is identified, add a permanent T721 regression to the normal suite,
prove it fails before changing behavior, then verify the same test passes and
that pointer operation recovers. Keep T721 unresolved until then. The T700
startup recurrence requires its existing mitigation acceptance separately.
