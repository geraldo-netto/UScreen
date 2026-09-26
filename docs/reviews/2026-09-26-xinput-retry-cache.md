# T625: retain unchanged XInput mappings during retries

T622 removed unnecessary explicit XRandR refreshes once output ownership was
known, but each `xinput map-to-output` still queried RandR resources. Upstream
xinput 1.6.4 calls `XRRGetScreenResources` in both `map_output_xrandr` and
`find_output_xrandr`, before setting the device transformation matrix. Repeating
successful touch/pointer mapping while waiting for the pen therefore retained
forced connector queries.
[Upstream source archive](https://xorg.freedesktop.org/archive/individual/app/xinput-1.6.4.tar.gz),
`src/transform.c`, SHA-256
`935c98d500486b687c02b2113e9e58d99efa13593cec3f93894616b6672ed1a4`.

The X11 adapter now keeps successful mappings only inside its existing retry
loop. It reads properties after a successful command and reuses that result only
when the current property readback matches. Snapshots require a device node and
a finite nine-value transformation matrix. Missing, oversized, malformed, failed
or changed readback causes another mapping attempt. A changed RandR report,
missing owned output, absent device or changed exact owned name invalidates the
cache. Mode/card/reconnect replacement starts with a fresh cache. Late devices
and unsuccessful mappings continue retrying; no failed command becomes cached
success. The normal mapper remains authoritative for rotation/reflection; this
change does not reproduce its matrix arithmetic or add a cross-platform device
API. Successful-command semantics are unchanged; unreadable properties simply
receive no caching benefit.

## Validation

The permanent T625 late-pen regression first failed with three touch remaps
instead of one, then passed. Normal input tests retain both-tablet ownership and
pen/eraser assertions and now include property readback commands. Added cases
cover topology/identity loss, changed matrices, command/readback failures,
invalid UTF-8 and bounded malformed matrix values. All 76 input tests pass;
related session tests supply real authentication/lifecycle coverage. All 1,258
Linux Rust functions meet 80%; changed input sources were recollected and only
byte-identical sources retain T627 counters. Complexity remains at most nine;
61 non-Linux functions remain explicitly unavailable under existing TODOs.

A bounded native run wrapped only `xinput`/`xrandr` command invocations. During
39 passes with the owned output present, Touch (id 21) and Pointer (id 23) each
ran `map-to-output` once, with 39 property reads each. The remaining 38 matching
readbacks per device reused the mapping. The base Pen (id 22) still failed all
39 attempts; it was not suppressed or reclassified. Compared with the previous
per-pass behavior, that avoids 76 redundant successful-device map commands in
this observed window. Earlier startup passes still needed explicit refreshes
while the output was absent. No CPU, total EDID-query or latency saving is
claimed from command counts alone. Physical stylus validation stays blocked and
skipped under T592.

Temporary host processes were stopped normally. No camera test or image capture
occurred. [Evidence](artifacts/2026-09-26-task-batch/t625/) retains command counts,
red/green tests, source hashes, coverage and the native log.
