# T558: EVDI startup delay investigation

The bundled libevdi 1.15.0 master-detection loop reproduces the observed delay:
**11.741 seconds elapsed, 6.736 seconds CPU**, ending with `Wait for master timed out`.
The probe only reads process metadata. It opens no DRM device, submits no ioctl,
attaches no display and leaves the live helper and Xorg unchanged.

`host/evdi/evdi_helper.c::acquire_capture_device_in` calls `evdi_open` before the
first reuse/connection message. In the bundled library,
`open_device` calls `wait_for_master` when Xorg is present. That function checks
other processes' `/proc/PID/fd` links and `/proc/PID/maps` for the device path.
Its nominal five-second limit counts 50 sleeps of 100 ms; up to 51 complete
process scans add their own CPU/wall time outside that nominal budget.

On this host Xorg PID 2512 runs as root. The ordinary UScreen user cannot read
its fd directory or maps. The library therefore cannot see that owner through
this heuristic. A probe copied the exact read-only detection functions from the
packaged source archive and substituted the existing helper PID for `getpid()`.
This excludes the live helper's already-open device, just as the real startup
scanner excludes itself, without closing that device. The loop exhausted its
attempts despite the existing working Xorg display.

This source-backed reproduction explains both the roughly 12-second delay and
roughly half-core CPU observed [during restart](../2026-09-20-evdi-restart/README.md).
It is not a captured stack trace of that historical invocation, so exact time
attribution to the original call remains inferred. Conversion workers start
after device acquisition and cannot account for this pre-connection interval.

The same wait structure remains in the [upstream library source](https://github.com/DisplayLink/evdi/blob/v1.15.0/library/evdi_lib.c).
No production code or dependency was changed. A safe mitigation must preserve
master acquisition, slave opening, exclusive helper leases, permission failures
and shutdown behavior; merely skipping the wait or making the daemon privileged
is not a validated fix. T558 remains open for that implementation, including a
permanent failing regression before changes and native ownership validation.

`master_probe.c.txt` preserves the exact one-shot probe, including upstream
license/copyright attribution; `master-probe.json` records timing and
`provenance.json` records source hashes and excluded PID. To repeat, copy it to
a temporary `.c` file, update the excluded live-helper PID, build with
`cc -O2 -Wall -Wextra`, and run as the normal user. Its `/proc` scan consumes CPU;
there is no need to repeat it while investigating idle capture.
