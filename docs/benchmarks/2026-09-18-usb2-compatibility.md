# T491 — USB 2.0 compatibility on the current tablet

**The current tablet already uses a real USB 2.0 connection, and the tested
1280×800/60 FPS H.264 configuration works over it.** No forced downgrade,
USB-controller change, cable swap or simulated bandwidth limit was needed.
This answers compatibility for the connected setup; it does not qualify every
USB 2.0 tablet or guarantee that every selectable stream fits the link.

## Connection evidence

ADB identifies the RugKing Pad 2 Pro at USB path `1-1.4`. Linux sysfs reports
`speed=480`, `version=2.00`, and `lsusb -t` shows that same device under the
480M branch of an xHCI controller, through a 480M hub. A storage device shares
that hub. The topology and device number stayed unchanged across the checks.
USB-C connector shape was not used to infer the negotiated data speed or the
tablet's maximum possible capability.

For this machine, the read-only checks are:

```bash
adb devices -l
lsusb -t
cat /sys/bus/usb/devices/1-1.4/speed
cat /sys/bus/usb/devices/1-1.4/version
```

Use the actual tablet path on another host. The kernel documents the device
attributes in its [USB sysfs ABI](https://www.kernel.org/doc/Documentation/ABI/stable/sysfs-bus-usb).
There is no reason to reset a working controller merely to reproduce a speed
that is already negotiated; that would also disturb other attached devices.

## Payload throughput

Three 64 MiB host→tablet transfers through ADB shell stdin measured
**241.56, 244.34 and 238.89 Mb/s**. Android computed SHA-256 over each complete
payload; every digest matched. Timing includes subprocess/ADB startup, transfer,
Android hashing and process exit, with the ordinary UScreen session active.
These are useful payload measurements, not a raw USB bus maximum. No deliberate
shared-hub storage workload was added.

For context, the preserved low-latency H.264 motion sample uses about 7.1 Mb/s
on average. Average headroom alone does not prove burst latency, so the next
test sends the actual paced encoded pictures through USB to the tablet decoder.

## Streaming and recovery trials

The existing isolated T479 replay APK was verified by SHA-256 before use.
Eight trials covered motion, pen and text at 60 FPS plus text at 5 FPS, each
twice. Every trial has two four-second phases with a controlled decoder
retirement/recreation between them. Each trial creates and removes its own ADB
reverse TCP route. The first second of each phase is excluded from steady
latency. The replay cancels on touch, focus loss or backgrounding.

Stock FFmpeg uses the shared production VAAPI Constrained Baseline/CAVLC policy,
quality 18, 1280×800 and the verified NV12 corpus. The replay requests app-only
50% brightness and 60 Hz; all eight results report 60 Hz. It does not replace
UScreen, alter its saved preferences or attach another EVDI display.

**All 2,960 submitted pictures received render acknowledgements.** All sixteen
captured encoded phases also decoded with stock FFmpeg without error-level
diagnostics and with exactly the expected total of 2,960 pictures.

| Workload | Raw submission→render-ACK p50, ms | p95, ms | p99, ms |
| --- | ---: | ---: | ---: |
| Motion, 60 FPS | 19.27 | 22.34 | 23.57 |
| Pen, 60 FPS | 19.31 | 21.87 | 24.51 |
| Text, 60 FPS | 19.77 | 22.14 | 23.39 |
| Text, 5 FPS | 23.22 | 26.88 | 27.90 |

Cells are medians of four phase percentiles using the existing T479 summarizer.
The host clock measures raw-write admission through encoding, the isolated USB
route, decoder callback and returned ACK. This excludes EVDI/compositor capture,
the production stream-server queue and physical pixel presentation. Sparse text
has fifteen post-warmup samples per phase, so tail estimates are coarse.
The [earlier optimized comparison](2026-09-18-profile-selection.md) reported
motion 19.04/22.25 ms p50/p95; the present series is close and does not establish
a new speedup or a USB 2.0 regression.

The tests exercise fresh replay connections and decoder recovery, not physical
unplug/replug, host-controller reset or a desktop attachment cycle. Those were
unnecessary for the requested speed check and would interact with unresolved
T222. The final snapshot confirms UScreen foreground and unchanged daemon,
capture helper, FFmpeg, Cinnamon and Xorg PIDs.

## Configuration and power implications

Keep the measured 1280×800/60 FPS, low-latency H.264 setup as a working choice
for this tablet. Actual content, decoder capability and shared-bus traffic still
matter at other resolutions/rates. Let users tune their settings; a universal
USB-version-based cap is not justified by this one setup. A VAAPI CQP stream is
not bitrate-capped, so its average fixture rate is not a maximum traffic bound.

The device descriptor advertises `bMaxPower=500mA`, and Android reports USB power.
Neither establishes measured input watts, charging negotiation or sustained net
battery gain. The current connection can carry the tested video while still
being inadequate to charge the tablet during use. T388 retains the matched
sustained battery comparison. No host/tablet power setting was changed.

The [evidence directory](2026-09-18-usb2-compatibility/) contains before/after
topology and health, checksummed payload results, the trial plan, exact replay
provenance, raw timing/Android results, all encoded streams, software decode
validation and checksums. T491 is resolved for this requested current-tablet
USB 2.0 check; no general multi-tablet campaign was performed or required.
