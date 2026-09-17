# Cinnamon session restart during UScreen connection

Reviewed commit: `6d73993`. Incident: 2026-09-17, Europe/Rome (CEST).

Xorg crashed while processing a RandR gamma request shortly after UScreen
attached an EVDI display. LightDM subsequently started another Cinnamon
session. The crash preceded UScreen's corrective resolution switch.
The exact Xorg failure trigger and a safe mitigation remain unverified (T222).

## Evidence

Sources: the current boot's journal, `coredumpctl info 2392`, and
`/var/log/Xorg.0.log.old`. Relevant evidence is transcribed below because
the original logs can rotate. Device serial numbers and session tokens are
omitted.

| Time (CEST) | Event |
|---|---|
| 01:52:24.916 | UScreen reports the USB tablet connected. |
| 01:52:24.978 | UScreen generates a 2960x1848@60 EDID with 310x194 mm physical dimensions. |
| 01:52:26.690 | Tablet reports 1280x800, 218x136 mm; UScreen publishes the new auto-resolution settings. |
| 01:52:26.698 | UScreen saves settings. |
| 01:52:39.400 | Helper announces its EVDI connection on card0. |
| 01:52:39.578 | UScreen reports that the compositor negotiated the old 2960x1848@60 mode. |
| 01:52:40 | Xorg's core timestamp records the fatal event. |
| 01:52:42.216 | UScreen warns that EVDI did not appear in kscreen-doctor within 3 seconds. |
| 01:52:42.217 | UScreen starts the encoder, immediately notices changed settings, and begins switching 2960x1848 to 1280x800. |
| 01:52:42.257 | The first helper exits cleanly. |
| 01:52:44 | systemd-coredump logs completion of Xorg's core dump. This is later than the crash timestamp. |
| 01:52:50.571 | Replacement helper announces its EVDI connection. |
| 01:52:53.711 | The new display server negotiates 1280x800@60. |
| 01:52:54 | The journal records the new Cinnamon session's services starting. |

The old Xorg log ends with:

```text
[401820.960] (EE) 2: /usr/lib/xorg/Xorg (miDCInitialize+0xb1d)
[401820.960] (EE) 3: /usr/lib/xorg/Xorg (miScreenInit+0x705)
[401820.960] (EE) 4: /usr/lib/xorg/Xorg (miScreenInit+0x139b)
[401820.961] (EE) 5: /usr/lib/xorg/Xorg (xf86DiDGAInit+0x6d7)
[401820.961] (EE) 6: /usr/lib/xorg/Xorg (xf86CVTMode+0x2249)
[401820.961] (EE) 7: /usr/lib/xorg/Xorg (ProcRRSetCrtcGamma+0xc2)
[401820.962] (EE) Segmentation fault at address 0x20
[401820.962] (EE) Caught signal 11 (Segmentation fault). Server aborting
```

The core records `SIGABRT` because Xorg aborts after handling the segmentation
fault. Most names above are the nearest exported symbols, not fully resolved
internal function names. Do not treat their offsets as source-level attribution.
The faulting executable offset is `0x1bf2ed`; disassembly of the installed
binary shows a read from `0x20(%rax)` there, consistent with the reported
near-null address. This does not identify the originating client or object.

Installed display components: `xserver-xorg-core 2:21.1.12-1ubuntu1.6`,
`xserver-xorg-video-amdgpu 23.0.0-1ubuntu0.24.04.1`, Cinnamon `6.6.9+zena`,
Muffin `6.6.3+zena`. Xorg's build ID is
`2dc631fda11e081777c5d43ba3ad1d6ce8a5a6fd`.

## Findings and required regression coverage

- **T222: Xorg crash during virtual-display attachment.**
  The connection immediately precedes the failure, but temporal proximity
  does not prove which display or input event created the invalid state.
  UScreen contains no Cinnamon-restart command or RandR gamma-setting call.
  A symbolized core or isolated reproducer is needed before attributing the
  fault to a particular component. The core file is inaccessible to the
  current user; Ubuntu's debuginfod returned HTTP 404 for this build's debug
  information. The normal tests use fake helpers and provide no isolated
  Xorg/EVDI session to reproduce a server crash. Keep this unresolved until
  an automated attach/gamma/detach regression and a verified mitigation exist.
- **T223: initial attachment uses stale settings.**
  `main.rs:1835` publishes tablet presence after ADB setup, before tablet
  geometry is received. `capture.rs:975` snapshots settings, then
  `while_active` watches only display presence and shutdown during setup.
  The settings notification is consumed only after setup enters the encoder
  session (`capture.rs:1265`). The timeline demonstrates an old EDID being
  connected after the new geometry was already received. This introduces an
  unnecessary hotplug cycle. Physical panel dimensions are also initialized
  to defaults on each launch, so saved pixel dimensions alone do not avoid
  corrective reconnection. Add delayed-helper/metadata regression coverage
  before fixing startup coordination; test first launch, daemon restart,
  and explicit fixed-resolution operation. Eliminating this race alone is
  not evidence that T222 is fixed.
- **T224: KScreen placement runs on an unsupported desktop backend.**
  `capture.rs:357-391` retries KScreen discovery 15 times without checking the
  desktop backend. It is called on every pipeline setup, including Cinnamon.
  Here the helper already reported a rendered mode, but capture still waited
  approximately 3 seconds and emitted a misleading missing-output warning.
  This conflicts with `docs/compatibility.md:21-26`, which reserves automatic
  placement for KDE Wayland and gives X11 placement to desktop settings.
  Add command-spy coverage proving X11 bypasses KScreen placement and the
  delay, while KDE Wayland retains placement, then gate calls accordingly.

## Validation performed

```text
cargo test -p uscreen --bin uscreen capture::tests::t091 -- --test-threads=1
2 passed; 0 failed
```

These existing tests exercise shutdown/detach cancellation with fake helpers.
They do not exercise settings changes during setup or real Xorg stability.
No live display configuration, daemon state, driver, or desktop setting was
changed, and the crash was not deliberately reproduced. This review adds
findings and evidence only; regression tests belong with the fixes.
