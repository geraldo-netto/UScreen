# T622 — cached X11 mapping queries

Input mapping reads `xrandr --current --prop` on every retry. If no output has a
valid, unambiguous EDID belonging to this tablet, it explicitly refreshes with
`--prop` on attempts 0, 4, 8, … (at most ten forced queries in forty retries).
Once ownership is known, delayed input-device creation requires no forced query.
DRM connector identity is reread each retry; foreign, disconnected, malformed or
ambiguous EDIDs retain their existing rejection behavior. Linux command handling
stays inside the X11 adapter; no configuration or wire contract changes.

Two permanent T622 regressions failed before the change and pass afterward:
late input on stable topology performs three cached queries and no refresh;
stale/foreign topology remains unmapped until the second explicit refresh makes
the owned EDID visible, with cached retries between refreshes. Both fixtures
contain another tablet's device and assert it is never mapped. Existing bounded
invalid-EDID, ownership and provider-renaming regressions remain intact.

All 72 input-module tests pass. Native LLVM counters pass the 80% per-function
gate for all 17 functions in `mapping.rs`; the whole-project cyclomatic gate
reports zero functions over nine. [Evidence](artifacts/2026-09-26-task-batch/t622/).

A normal daemon start followed by stop/start reconnected the existing tablet,
created its EVDI output and mapped Touch/Pointer to `DVI-I-2-1`. XInput readback
confirmed matrices 0.25 horizontally and 0.370370 vertically against the
5120×2160 desktop, matching the 1280×800 tablet at the left edge. Encoder output
resumed. A logging XRandR adapter retained the actual cached/forced calls.

The base pen remains an Xorg keyboard until its tool behavior can be checked;
its failed mapping keeps retries active. Physical stylus acceptance remains
blocked and skipped under T592. Native timing here is startup/reconnect evidence,
not a new CPU or latency comparison.
