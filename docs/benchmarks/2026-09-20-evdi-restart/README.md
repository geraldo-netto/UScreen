# T556: EVDI restart and live playback observations

The maintainer requested one normal UScreen service restart and retained data,
including a successful run. The T554 horizontal-span conversion helper became
active. Xorg survived and Android stayed awake. Playback initially resumed,
then our diagnostic collector caused a separate ADB outage; restoring the two
display reverse mappings recovered video, confirmed by the maintainer.

## Setup and scope

- Linux host, USB RugKingPad2Pro, Android 16; YouTube in Linux Chrome on the
  tablet display, audio through Linux Bluetooth headphones (BW01 A2DP/SBC).
- 1280×800, 30 FPS, VAAPI Constrained Baseline, quality/CQP 18, 4 MiB capture
  pipe, automatic conversion workers, automatic resolution, no profile cache.
  `settings.json` contains the selected settings; its `bitrate_kbps: null` is a
  collector lookup of a nonexistent config key, not an observed zero bitrate.
- One `systemctl --user restart uscreen.service`; no module unload, Android
  lock/power command, force-stop or intentional ADB server reset.
- Before/after daemon versions differ as well as the helper. YouTube content
  progressed throughout. This is operational evidence, not an isolated paired
  optimization benchmark. See the [controlled conversion measurements](../2026-09-20-damage-regions/README.md).

## Timeline (UTC, 2026-09-20)

| Time | Evidence/event |
| --- | --- |
| 20:26:02.868 | Service restart requested. |
| 20:26:03.457 | Restart command returned success; this is not video recovery time. |
| 20:26:06.883 | First new video connection. |
| 20:26:11.886 | Five-second wait for codec configuration expired. |
| 20:26:17.402 | Video connection retried. |
| 20:26:18.615 | Helper reported EVDI reuse/connection. |
| 20:26:18.828 | Host sent codec configuration, about 16 seconds after restart request. |
| 20:26:23.853 | First aggregate render-ACK report confirmed resumed frames. |
| 20:27:09.063 | Diagnostic unit completed; its descendant ADB server disappeared. |
| 20:27:10.126 | Android began repeated localhost:8890 `ECONNREFUSED` errors. |
| 20:30:04 | Restored owned reverse mappings, without another EVDI restart. |
| 20:30:04.909 | Recovered stream delivered IDR; control reconnected immediately after. |

The collector ran in a transient systemd user service. Its `adb devices` command
started the new ADB server while the old UScreen service stopped. That server
belonged to the collector's cgroup and did not survive collector completion.
Both reverse mappings were missing, while the host still listened and encoded.
This collector lifecycle mistake caused the loading screen; it was not an
observed failure of NV12 conversion. Recovery only restored tablet TCP 8890 and
8891 to their corresponding host ports. Future collectors must not own shared
ADB servers whose lifetime they terminate. T557 tracks automatic missing-route
repair in the host; its permanent regression simulates the same lost mappings
without disrupting a real device.

## Measurements

| Metric | Before | After, steady playback |
| --- | ---: | ---: |
| Capture helper CPU, percent of one core | 5.62% | 5.60% |
| FFmpeg CPU, percent of one core | 2.29% | 2.30% |
| Capture-to-FIFO, median of window p50 | 47.05 ms | 47.05 ms |
| Packet-ready to render ACK, median of window p50 | 17.0 ms | 17.0 ms |
| Tablet arrival to render callback, median of window p50 | 12.4 ms | 12.3 ms |
| PipeWire ERR counters in short snapshots | 0 | 0 |

CPU before covers 15.292 seconds. Post-startup CPU uses the last 30 process
snapshots, 20:26:37.131–20:27:06.748 (29.617 seconds). For matching PID and start
time, CPU percent is `100 × delta(cpu_ticks) / (100 × elapsed_seconds)`; this
host's clock tick rate is 100 Hz. The original whole-window `after-cpu.json`
reports helper CPU at 15.01%, including startup work, and must not be compared
as steady-state conversion cost.

Latency values summarize the five-second report windows in the retained service
journal, grouped by old/new process IDs. Before has 28 windows per metric;
after has 10 capture windows and 9 ACK/tablet windows. These are medians and
ranges of reported window percentiles, not pooled sample percentiles. The
metrics cover different pipeline segments and cannot be summed into a measured
end-to-end latency. [summary.json](summary.json) retains counts, ranges and CPU
calculations. The encoder reports roughly 30 access units per second after
startup and recovery.

This full-video sample shows no material latency or steady CPU change. Sparse
damage can save conversion work in the controlled benchmark; full-screen video
changes much of the image. Bluetooth audio gaps were not reproduced here, so
T414 remains open. Successful restart does not resolve historical Xorg failure
T222 or prove reliability across other machines.

## Remaining findings and retained evidence

- T558: helper existed around 20:26:06 but connected to EVDI at 20:26:18,
  using roughly half a CPU core in the gap. Investigate startup before attributing
  this to any capture/conversion stage, which had not begun streaming.
- T559: config-change logs compare duplicate bare TOML keys across sections;
  alternating camera/display dimensions and bitrates do not prove preferences
  changed. Investigate and test the diagnostic comparison independently.
- Xorg PID 2512 and process start ticks remained unchanged. Its executable hash
  was unavailable to the unprivileged collector; process snapshots omit that
  hash. `xorg-restart-delta.log` contains the actual appended Xorg log, including
  ordinary EDID and framebuffer resize events. No new coredump was reported.
- `*-processes.json` retain binary hashes. Live helper SHA-256:
  `7550151bc674a7829c756c70d2e25dc9b4cddca1319a4406e86a6d35adc77cd8`.
  Installed AppImage SHA-256 at restart:
  `0996b7ca196a4988c936b91e7ca9b24674b98187b67abbb83542fac777101b6b`.
- Compressed service journal and process samples retain original observations;
  audio, Android power, ADB, display, service and restart command records retain
  timestamps, exits and selected output. `recovery-journal.log` records the
  restored stream; `android-forwarding-errors.log` records the failed route.
- Kernel output is filtered to relevant EVDI/DRM/GPU/Xorg/crash lines, excluding
  unrelated application audit paths. Screenshots, full configuration and raw
  unrelated logs remain local and are not part of this report.

The prior AppImage remains installed alongside the new one as
`UScreen-before-T554-57b42929cf2a.AppImage` for rollback. No Android update was
needed for T554 or the display forwarding repair.
