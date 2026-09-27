# T644 diagnostic readiness

The historical Ubuntu-runner Xvfb timeout has no retained child/syscall state.
Both fresh Linux coverage configurations, the full script suite and the focused
17-test visibility suite pass locally; this does not identify the old cause.

Permanent regressions first failed for missing live-child timeout metadata and
for inspecting an already-reaped PID. They pass after bounded PID/exit/stderr,
status, wait-channel and syscall diagnostics were added before child cleanup.
Exited processes are not inspected again. Unreadable proc fields remain explicit.
The five-second deadline and all focus/geometry/occlusion assertions are unchanged.

T644 stays unresolved and blocked on a runner recurrence retaining this evidence,
or an equivalent deterministic reproduction. A causal fix still requires its own
permanent failing regression. No new remote workflow or push was requested.
