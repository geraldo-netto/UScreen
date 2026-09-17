# Device benchmark artifacts

The dated packetizer and control JSON files describe isolated synthetic tests.
Device runs are stored in directories containing build/deployment provenance,
phase boundaries, raw observations, a calculated summary and a timeline plot.
Results are scoped to their recorded build, hardware and workload.

The [2026-09-17 device baseline](2026-09-17-device-baseline.md) is the initial
physical-tablet run for the next optimization comparison.

## Repeating the device baseline

Build/install both applications first. Keep the Linux release profile and the
Android APK variant/signing configuration identical between comparisons. Record
the source commit and SHA-256 of the daemon, GUI, helper, libevdi and APK. The
default CLI encoder uses the installed stock FFmpeg; record that version too.

The initial workload requires Linux X11, Python 3 with Tk, an existing
non-primary 1280×800 virtual display and one connected Android tablet already
running a debuggable UScreen APK (`run-as` reads the app's process counters).
The collector does not create displays, change settings or
install applications. Its borderless test window covers only the selected
monitor; closing the window or terminating the collector ends the experiment.
Keep the tablet on UScreen and leave the USB cable, hub and power source unchanged.

```sh
# Set TABLET_SERIAL to the device shown by adb devices -l.
python3 scripts/benchmarks/run-baseline.py \
  --serial "$TABLET_SERIAL" --geometry 1280x800+3840+0 \
  --output /tmp/uscreen-device-baseline --seconds 300 --warmup 60
python3 scripts/benchmarks/summarize.py /tmp/uscreen-device-baseline \
  > /tmp/uscreen-device-baseline/summary.json
python3 scripts/benchmarks/plot-baseline.py /tmp/uscreen-device-baseline \
  /tmp/uscreen-device-baseline/timeline.png
```

Use a new output directory each time. Adjust the monitor position to the actual
`xrandr --listmonitors` result. The workload has fixed 1280×800 content; a different
resolution needs a separately recorded workload rather than silently scaling
this one. Plotting requires matplotlib; collection and summarization use the
Python standard library plus Tk.
The initial collector uses Android clock-tick/page-size constants of 100 Hz and
4,096 bytes, verified with `adb shell getconf CLK_TCK` and `getconf PAGESIZE` on
this tablet. Verify and adapt these before using different hardware; T382 retains
portable unit discovery as part of broader device coverage.

The 36-minute sequence is three paired trials: static/motion, motion/static,
static/motion. Each five-minute measured phase has a one-minute warm-up. Static
content is a fixed text/grid pattern. Motion scrolls the same text at 96 pixels/s
and moves four colored rectangles, scheduled at 60 updates/s. Scheduling counts
describe requested drawing updates, not display presentation. The script records
actual phase timestamps and source hashes. The application remains unmodified.

## What is recorded

| Artifact | Meaning |
| --- | --- |
| `metadata.json` | Source/workload identity, phase plan, host clock-tick units, kernel and Android build. |
| `deployment.json` | Build profile, installed artifact hashes, settings, device/driver/USB information and validation evidence. Supplement per run with its deployment facts. |
| `phases.jsonl` | Actual host UTC/monotonic phase boundaries and workload update counts. |
| `samples.jsonl.gz` | Five-second process CPU ticks/RSS/threads and host load; thirty-second Android battery/thermal/display observations; minute app memory snapshots. |
| `host-windows.jsonl.gz` | Filtered, original-timestamp performance messages from the service journal. |
| `android.log.gz` | Filtered decoder/statistics messages; no screenshots, input coordinates or tokens. |
| `summary.json` | Per-phase CPU/RSS distributions, charge change and distributions of log-window statistics. |
| `timeline.png` | Battery change, host pipeline CPU and logged latency windows across all phases, including warm-up. |
| `run-integrity.json` | Coverage/continuity checks, recovery provenance and validation evidence. |
| `SHA256SUMS` | Hashes of the archived artifacts. |

Raw line files can be kept uncompressed while collecting. The summarizer accepts
either raw JSONL or `.jsonl.gz`. Preserve raw files and hashes; a summary alone
cannot support later reanalysis. Journal filtering accepts text and byte-array
MESSAGE representations (T410), strips ANSI colors and redacts 64-digit hex
identifiers. The normal tooling suite runs permanent T382/T410 parser/unit tests.

## Interpretation and comparison

CPU is computed from process user+system tick differences divided by elapsed
host monotonic time and clock ticks/second. **100% is one logical core.** PID and
start-time identity must match; a replacement process is not treated as negative
CPU. The host pipeline sums daemon/helper/FFmpeg and excludes the workload,
Cinnamon and Xorg, which are recorded separately. Android app CPU excludes
separate codec/compositor/system processes. RSS includes shared pages; do not
interpret summed RSS as unique physical memory.

The host's `encode→display` label measures **encoded access unit ready → render
acknowledgement received**. It excludes capture, encoding and packetizer assembly.
The Android timing ends when its render callback runs; callbacks can be delayed
or batched, so it is not optical input-to-photon latency. See the
[MediaCodec callback contract](https://developer.android.com/reference/android/media/MediaCodec.OnFrameRenderedListener).
The helper's `capture→fifo` interval starts after the framebuffer grab. Never add
their independently reported percentiles to estimate end-to-end latency.

Logs contain five-second window percentiles, not per-frame raw durations. The
summary therefore gives distributions **of those window statistics**, including
their median/min/max and window count. A median of window p95 values is not a
pooled frame p95. No frame-level p99 can be reconstructed. The first ten seconds
of log reports after each measured-phase boundary are excluded to avoid mixed
warm-up windows. Encoded FPS is access units divided by the logged interval;
it is not proof that the panel presented every frame.
Reports also have bounded sample storage: the helper retains at most 256
capture-to-FIFO samples per report, and the host latency tracker at most 1,024
acknowledgement samples. These are the implementation's logged statistics,
not an independently sampled trace of every frame.

Android's charge counter uses microampere-hours. The summary computes
`net mA = charge_delta_uAh × 3.6 / elapsed_seconds`; a negative result means the
battery lost charge despite USB power. Report endpoints, duration, temperature
and gauge resolution. This is whole-device **net battery current**, not USB input
current or isolated app power. The reported maximum charging current/voltage is
not a wattmeter reading. See [BatteryManager](https://developer.android.com/reference/android/os/BatteryManager#BATTERY_PROPERTY_CHARGE_COUNTER)
and [Perfetto power-data limitations](https://perfetto.dev/docs/data-sources/battery-counters).

Compare the same static/motion trials at the same resolution, encoder, quality,
rate policy, app brightness, observed refresh, USB route, charging source and APK
variant. Check process continuity, foreground state, temperature and background
load. Repeat runs and report variability; a later run on a different charging
port is a separate power experiment. Small changes below gauge/timing variation
are inconclusive. The full T382 multi-device/stage-trace matrix remains separate
from this single-tablet baseline.
