# Linux capture pipe buffer

Open **UScreen → Video → Capture pipe buffer** on Linux. Choose **1, 2, 4 or
8 MiB** and click **Apply**. This is the raw-video buffer between the Linux
capture helper and FFmpeg, not Android memory or a total application RAM limit.
The setting has one owner and is deliberately absent from Android settings.

The default remains **1 MiB**. Each active tablet has its own pipe with the same
requested capacity. The GUI shows the actual kernel capacity separately for
each tablet. A saved request is not proof that Linux granted that much space.
Inactive capture shows “waiting for capture”; the GUI reads helper reports and
never opens the capture pipe.

A pipe-only edit applies to the running helper without restarting UScreen,
FFmpeg or the virtual display. The helper checks for updates approximately once
per second, between complete frame writes, and retries a refused request. A
shrink can remain pending while queued bytes occupy more space than the requested
capacity. It never truncates queued frames to force a resize. If other settings
in the same edit require a restart, the button still says **Apply & restart**.

## Allow requests up to 8 MiB

First read and record the existing ceiling:

```sh
cat /proc/sys/fs/pipe-max-size
```

The value is bytes: **1048576 = 1 MiB**, **8388608 = 8 MiB**. If the existing
value is already 8388608 or higher, do not lower it. This ceiling applies to
unprivileged processes across the host, including applications other than
UScreen. Raising it permits larger requests; it does not immediately allocate
8 MiB to every pipe or select 8 MiB in UScreen.

To raise a smaller ceiling until reboot, run in a terminal:

```sh
sudo sysctl -w fs.pipe-max-size=8388608
```

Then select the desired capacity in UScreen and **Apply**. If that request was
already saved, the active helper retries it automatically. Confirm the
**effective capacity** in the GUI; no display restart is required.

For persistence on distributions that load `/etc/sysctl.d` during boot, create
this dedicated file (inspect it first if it already exists):

```sh
printf 'fs.pipe-max-size = 8388608\n' | sudo tee /etc/sysctl.d/90-uscreen-pipe.conf
sudo sysctl -p /etc/sysctl.d/90-uscreen-pipe.conf
```

The second command loads only this file. Other sysctl files can override its
value at boot; check the live `/proc` value again after reboot. On distributions
with a different boot-time sysctl mechanism, use that mechanism to load this
setting. UScreen does not require systemd for the runtime resize and does not
run these privileged commands itself.

Linux also enforces aggregate per-user pipe-memory limits. An increase may still
be refused even after raising the per-pipe ceiling. Inspect the limits with:

```sh
cat /proc/sys/fs/pipe-user-pages-soft
cat /proc/sys/fs/pipe-user-pages-hard
getconf PAGESIZE
```

Those two limits are in pages, not bytes. Zero denotes no corresponding limit.
Other applications consume the same user's allowance. A new pipe first attempts the existing 1 MiB baseline, then the selected size.
UScreen keeps streaming with the capacity already granted; its helper log records requested/effective
bytes and a resize error number when the result changes. It does not obtain
extra capabilities or change these system limits automatically.

The [Linux pipe-size API](https://man7.org/linux/man-pages/man2/F_GETPIPE_SZ.2const.html)
documents permission failures, rounding and occupied-shrink refusal. See also
[sysctl](https://man7.org/linux/man-pages/man8/sysctl.8.html) for applying and
loading kernel parameters, and [pipe limits](https://man7.org/linux/man-pages/man7/pipe.7.html)
for the per-user accounting rules.

## Undo

Choose **1 MiB** in UScreen and **Apply** first, then wait for its effective
capacity to drop. Remove the dedicated persistent file only if you created it
for this purpose:

```sh
sudo rm /etc/sysctl.d/90-uscreen-pipe.conf
```

Restore the original ceiling you recorded earlier. For example, if it was
1048576 bytes:

```sh
sudo sysctl -w fs.pipe-max-size=1048576
```

Restoring the ceiling does not shrink existing pipes in other applications.
Do not replace an administrator's larger original ceiling with this example.

## Choosing a size

Larger is not inherently faster to display. In the isolated
[capacity ramp](benchmarks/2026-09-17-pipe-capacity.md), 2 MiB reduced transfer
work for 1280×800 NV12 frames, while 8 MiB helped the larger 2960×1848 frames.
Extra capacity also allowed more old video to accumulate under a slow reader.
Those tests measured pipe transfer, not whole-application display latency.
Keep 1 MiB unless a controlled comparison demonstrates a useful improvement for
your workload; available RAM alone is not evidence for choosing 8 MiB.

Four 8 MiB pipes can hold 32 MiB of kernel payload capacity in total, separate
from capture buffers and encoded-video queues. The GUI reports capacity rather
than how many bytes are currently occupied.

## Configuration and implementation

The persisted Linux setting is `pipe_capacity_mib = 1` in `config.toml`, with
allowed values 1, 2, 4 and 8. Unsupported integers fall back to 1. Manual TOML
edits are loaded on daemon startup; use the GUI's Apply for a live change.

At startup the daemon publishes the saved preference in a private atomic
`pipe-capacity-mib` request file beside its capture FIFOs. The GUI publishes the
same file after its configuration transaction commits. Every helper receives
the file path and reads it on FIFO open and at its bounded frame-boundary check.
Capacity reports are bound to the helper PID/start time and FIFO inode so stale
reports cannot describe a replacement stream. A replacement FIFO after
partial-frame recovery receives the current preference;
no captured display has to be recreated for a size edit.

Permanent T415 tests cover persisted choices/defaults, invalid input, save/apply
ordering, GUI selection, actual kernel capacity, denied increases, busy shrink,
queued-byte preservation, reopen, rounding, stale status, and inspection while an
encoder waits for its writer. Busy-shrink tests scale only the
resize syscall's byte argument to exercise real Linux pipes below an
unprivileged CI ceiling. The existing T226 partial-frame recovery tests remain
in the normal suite. No privileged system changes are required by these tests.
