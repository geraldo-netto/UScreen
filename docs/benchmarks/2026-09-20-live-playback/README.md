# T560: passive current-playback profile

One 60-second Android Perfetto trace plus concurrent Linux process/audio
observations, 2026-09-20 21:00:22–21:01:27 UTC. The existing app, EVDI helper,
encoder and ADB server remained running. No package replacement, foreground
change, lock command or service restart occurred. The maintainer confirmed
the selected rate is **30 FPS**; capture stayed 1280×800/30, with the tablet
panel refreshing around 60 Hz. The panel rate is not the video stream rate.

## Findings

| Observation | Measured result |
| --- | --- |
| Android app CPU, sum across threads | 38.007% of one core; device has eight cores |
| Busiest app thread, `MediaCodec_loop` | 12.471% of one core |
| Receiver `DefaultDispatch` thread | 4.321% of one core |
| `uscreen-render` thread | 3.313% CPU; about 55.32 seconds sleeping/waiting |
| Render-thread runnable wait per event | p95 0.400 ms; max 2.298 ms |
| Callback-thread runnable wait per event | p95 0.381 ms; max 0.752 ms |
| Native output-dequeue calls | 5,640, roughly 94/s; p50 10.72 ms elapsed, largely waiting |
| Native queue-input calls | 1,784; p50 2.07 ms elapsed, p95 2.84 ms |
| Video Surface buffer queues | 1,787 across roughly one minute |
| Interior queued buffers / present fences | 1,727 / 1,721 |
| Queue-to-present-fence interval | p50 24.14 ms; p95 31.25 ms; max 32.75 ms |
| App heap-size counter | 4,663–9,687 KiB; allocation call stacks/rate not captured |
| PipeWire ERR counters | All sampled counters stayed zero |
| Audio/Bluetooth service journals | No entries in the collection window |

No CPU saturation or dominant runnable-thread delay is evident in this sample.
Output dequeue wall time is chiefly waiting, not CPU consumption. Additional
decoder workers are therefore not justified by these observations. Codec/vendor
and compositor work remains a larger measured component than our render-thread
CPU, but this is attribution, not proof that a particular optimization will help.

Six interior video buffers were queued without a subsequent latch or present
fence. T561 retains their exact frame numbers and timestamps in `summary.json`
for correlation with packet/codec/compositor timing. Excluding one second at
each trace edge avoids classifying an unfinished boundary frame as omitted.
No recorded Perfetto error or data-loss statistic was nonzero. This supports
the observed queue/latch distinction; it does not identify why those buffers
were not selected for presentation.

No application GC slice was observed. The recorded 148 ms concurrent GC and
1.93 ms mutator pause belonged to `system_server`, not UScreen; `gc_owners.csv`
preserves that ownership. Heap size and RSS are not allocation rate or proof
against leaks. A method/allocation profile would be separate evidence if later
needed; no profileable APK was installed for this passive run. App logcat
contained no usable PID-scoped entries, so codec timing uses existing host ACK
reports and Perfetto rather than invented app log observations.

Host process CPU across 61.78 seconds was about 7.30% of one core for the helper,
2.22% for FFmpeg and 0.37% for the daemon. The 13 host report windows had median
window p50 values of 18.1 ms packet-ready-to-render-ACK and 13.1 ms tablet
arrival-to-render-callback. These are summaries of window percentiles, not pooled
percentiles; neither is physical presentation latency. They cannot be added to
the Surface queue interval because they use different boundaries and clocks.
Tracing/probing adds overhead and content was uncontrolled; this is not an A/B
performance comparison. Audio gaps were not established by this sample and
T414 remains open for correlation with a recurrence.

## Evidence and reproduction

`trace.pbtxt` requests scheduler wake/switch events, process counters, app atrace
graphics/video/ART events and SurfaceFlinger frame data. Scheduler meanings follow
the [Perfetto scheduling contract](https://perfetto.dev/docs/data-sources/cpu-scheduling).
FrameTimeline's 69 on-time app UI frames concern the Activity UI, not all 1,787
video buffers; video presentation uses `frame_slice` for SurfaceView BLAST layer
788. A present fence is compositor evidence, not an optical measurement.

The raw trace remains local at `target/t560-profile/playback.pftrace` because it
contains other device-process metadata. Its SHA-256 is in `complete.json`.
Selected SQL results, queries, compressed frame events, host samples/journal,
audio counters, memory snapshots and immutable collection/analysis script copies
are retained here. Processor: Perfetto v58.2 (`add693d8b`); tablet service v49.0.
`provenance.json` identifies the live binaries and selected settings, distinct
from the newer forwarding-repair package installed for the next normal start.

T560's narrow profiling assessment is complete. The evidence favors coordinated
idle work reduction (T492) over adding Android workers to current playback;
T561 and T414 preserve the unresolved presentation/audio findings.
