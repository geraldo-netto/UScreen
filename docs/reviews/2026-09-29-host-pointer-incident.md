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

Prepare a bounded observation that records mouse-only event counts/timestamps,
XInput delivery, pointer/button state, and compositor responsiveness without
grabbing devices, injecting input, collecting keyboard input, or changing saved
preferences. If failure recurs, collect it while keyboard control still works
and compare physical receiver events with Blent virtual input state. A fast
X query alone does not prove that mouse events reach clients or get rendered.

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
