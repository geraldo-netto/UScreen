# T623 — recovery warnings and duplicate grabs

Keep the 250 ms watchdog and one-second fallback. Coalesce them when both expire
on the same loop iteration: the watchdog grab now satisfies the fallback timer.
Previously this sequence immediately grabbed twice without processing a new
update between those calls. No kernel logging level or normal capture cadence
changes.

The inspected [EVDI v1.14.15 painter implementation](https://github.com/DisplayLink/evdi/blob/3dafd623f5c59ce6fe53f0662107d3e88f868de3/module/evdi_painter.c)
explains both historical warning classes. `evdi_painter_grabpix_ioctl` warns when
`was_update_requested` remains set; grabbing does not clear it.
`evdi_painter_request_update_ioctl` warns and ignores another request while that
flag is set. Normal dirty-update delivery clears the flag. Blent's watchdog
clears its local pending state to attempt recovery, so a grab followed by retry
can produce both warnings when the kernel still owns the original request.

An initially considered change to retain local pending state indefinitely was
rejected: if the kernel sent readiness but its event was lost, no future request
would be made and normal capture could remain stalled. The API does not expose
whether an earlier request is still pending. Retain the bounded, idempotent
retry; warning counts alone do not justify removing this recovery path. The
coalescing fix removes the demonstrably duplicate grab without that regression.

The permanent `t623_watchdog_coalesces_grabs_and_recovers_lost_events` regression
failed on the old double-grab sequence, then passed. It also checks retry after
lost readiness, delayed update-ready handling and the independent fallback.
Existing T294 no-spin/deadline assertions are unchanged. All 43 helper tests pass;
the native GCC coverage gate passes all 153 maintained capture functions at the
80% threshold. The project complexity gate reports no function above nine.

Two 20-second native helper observations, before and after, retained a working
1280×800 libx264 stream. Helper CPU was 1.17 and 1.21 seconds respectively; capture
callbacks and capture-to-FIFO samples continued. These were uncontrolled desktop
windows with concurrent test/build load, not a performance comparison. Neither
window reproduced a watchdog expiry or either warning class; their absence is
not proof of a warning-rate improvement. A startup fallback was observed in both
instrumented logs. Source-level warning conditions and the deterministic sequence
regression establish the fix; the historical individual warnings have no retained
call-site trace and are not individually attributed.

[Evidence and temporary instrumentation patches](artifacts/2026-09-26-task-batch/t623/)
retain the observations and hashes. Residual connector-query traffic during
repeated input mapping is a separate follow-up, T625. No native display or camera
session is left running by these checks.
