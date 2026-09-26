# T615 — EVDI setup investigation

The supplied messages do not establish a broken or repeated Blent setup.
Keep the stock driver and existing idempotent setup. A separate, measurable
opportunity is reducing forced connector queries during input mapping (T622);
update/grab warning attribution is retained as T623. No kernel warning was
hidden, no live DRM device was unloaded and no privileged driver change was made.

## Matched implementation and environment

Running kernel: 7.0.0-31-generic; EVDI reports **1.14.15** through sysfs and the
boot journal, with two devices and log level 4. Ubuntu's prebuilt module package
is `linux-modules-evdi-7.0.0-31-generic` 7.0.0-31.31~24.04.1. Xorg is
21.1.12 (2:21.1.12-1ubuntu1.6); the AMD DDX is 23.0.0 and libdrm-amdgpu
2.4.134. Bundled stock libevdi reports **1.15.0** through `evdi_get_lib_version`;
its binary hash is retained. Version numbers differ; the active capture path
works, and these logs do not demonstrate a library/driver ABI mismatch.

Upstream tag v1.14.15 resolves to `3dafd623f5c59ce6fe53f0662107d3e88f868de3`.
Downloaded pinned sources match both warning names and exact line numbers
490/1469. The upstream-source URL/hash manifest and extracted journal window are
in [evidence](artifacts/2026-09-26-evdi/). Kernel distribution patch equivalence
beyond those inspected paths is not claimed.

## What the messages mean

| Message | Source and implication |
| --- | --- |
| Imported object mapping rejected, owner amdgpu | `evdi_gem_mmap` rejects objects with `import_attach` and returns `-EINVAL`. This is an explicit restriction on that mapping path, not proof that all AMD capture/import failed. The later mode notification and working capture demonstrate the session continued. |
| Connector connected / EDID property set | `evdi_detect` logs on a successful detection call; `evdi_get_modes` updates the EDID property and logs on a mode query. These are query logs, not connection-generation counters. |
| Ignored DDC/CI address 0x50 | The driver's DDC/CI handler accepts address 0x37. Other addresses return false immediately, before the DDC/CI response wait; the warning does not imply eight 50 ms timeouts. Connector modes use the supplied EDID copy, independently of this rejected request. |

Primary code:
[import mapping](https://github.com/DisplayLink/evdi/blob/3dafd623f5c59ce6fe53f0662107d3e88f868de3/module/evdi_gem.c#L488),
[connector query callbacks](https://github.com/DisplayLink/evdi/blob/3dafd623f5c59ce6fe53f0662107d3e88f868de3/module/evdi_connector.c),
[DDC/CI admission](https://github.com/DisplayLink/evdi/blob/3dafd623f5c59ce6fe53f0662107d3e88f868de3/module/evdi_painter.c#L1468).

The retrieved 40594.5–40595.5-second journal window contains **one** actual
`Connected with Task` for evdi_helper PID 760746, **25** connected-query logs,
**24** EDID-property logs, **one** imported-map rejection, **eight** ignored
DDC/CI messages and **zero** double-connect warnings. The original short excerpt
omitted the earlier helper connection. Code agrees: `host/evdi/evdi_helper.c`
calls `evdi_connect` once after opening its owned card and reading the EDID.
The caller issuing the rejected mmap/I2C requests was not traced; attributing
them to a specific desktop process would exceed the evidence.

## Existing setup and measured query opportunity

`scripts/setup-evdi.sh` loads without unloading, reads current capacity, adds only
the missing devices and confirms the resulting count. Existing T269/T497 tests
exercise every setup entry point, live-device preservation, failures and invalid
capacity. All four tests passed; no artificial tests were added for this research.

Three interleaved read-only pairs on the current layout:

| Query | Median elapsed | EVDI connector + EDID logs per call |
| --- | ---: | ---: |
| `xrandr --prop` | 18.76 ms | 1 + 1 |
| `xrandr --current --prop` | 11.45 ms | 0 + 0 |

Captured outputs were byte-identical. Three short 250 ms idle controls emitted
none of these messages. The queries changed no display settings or EVDI
connections. This is a small stable-layout observation, not a startup or overall
CPU benchmark. `map_x11_devices` currently forces `--prop` on up to forty retries
at 250 ms spacing, so a cached-query policy is worth testing. T622 requires
late-connection/EDID freshness and ownership regressions plus native reconnect
validation before changing that behavior.

An earlier ten-minute kernel window, including normal activity/reload, also
contained 34 grab-without-ready pairs and 25 already-requested warnings.
`capture.c::recover_capture_if_stalled` deliberately contains a 250 ms watchdog
and one-second fallback; T623 asks whether those warnings reflect needed recovery
or avoidable requests. Counts alone neither establish wasted CPU nor justify
removing safeguards. T558's stock-libevdi decision and T222's deferred crash
investigation remain unchanged.

Full journal and pinned-source copies are archived locally at
`~/.local/share/blent/profiles/2026-09-26-evdi/`; the repository retains the narrow
supporting excerpts, measurements, provenance and setup-test result.
