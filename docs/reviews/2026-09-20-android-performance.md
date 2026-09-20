# Android performance assessment — T560

UScreen already overlaps network receipt, decoding/output and control work.
Further parallelism may help a measured bottleneck, but existing device
experiments do not justify increasing worker counts for normal video playback.
This review changed no Android runtime defaults and ran no disruptive device
experiment.

## Current implementation

| Area | Implemented behavior | Boundary/limitation |
| --- | --- | --- |
| Concurrency | `VideoReceiver` receives on `Dispatchers.IO`; `DecoderSession` has a dedicated output thread and frame-callback HandlerThread; `ControlSession` uses its own asynchronous connection/lifecycle. | Ordered access units still enter one codec session. More workers do not establish faster codec output. |
| Decode/render | `DecoderCapabilities.decoderName` prefers compatible advertised hardware decoders; `DecoderSession.configureStartup` configures output to the display Surface. | Explicit selection can choose another compatible decoder. No application-level raw-pixel conversion loop exists in the normal display path. |
| Buffers | `VideoPacketReader` reuses a 512 KiB initial payload array, grows only when needed, and dispatches borrowed slices. | The synchronous path still copies compressed payload into codec storage; small metadata allocations remain. |
| Data structures | `FrameTiming` uses bounded primitive arrays and a validated sequence-to-slot cache, with bounded collision fallback. | Host-JVM bookkeeping measurements do not establish Android ART or display latency improvements. |
| Backpressure | Video receive buffer request is 128 KiB; synchronous input retries are bounded. Experimental `DecoderMailbox` admits at most two owned access units with a shared deadline. | Current default is synchronous input/output, not callback mailbox mode. Additional buffering can retain older frames. |
| Vectorization | Normal display decoding/rendering is delegated to Android's codec and Surface path. | No custom Android NEON/SIMD pixel kernel is implemented. Host C conversion vectorization is separate; vendor codec internals are not measured by this review. |
| Scalability | Bounded packet sizes, timing history, deadlines and lifecycle ownership constrain session resources. | This is not proof of simultaneous display/camera/audio capacity or arbitrary higher FPS/resolution. User-controlled settings and measured device limits remain necessary. |

The [MediaCodec contract](https://developer.android.com/reference/android/media/MediaCodec)
supports decoding to a Surface and separate asynchronous callback operation;
neither interface promises a particular latency or acceleration gain.

## Existing measured experiments

The [T386 decoder-profile trials](../benchmarks/2026-09-18-decoder-profiles.md)
used this tablet with fixed 1280×800 H.264 fixtures in a separate replay app.
For sparse motion at 5 FPS, callback mode reduced CPU from the candidate
synchronous path's 7.85% to 4.48% of one core. At motion/60 FPS, the comparison
was 41.89% versus 41.63%, while callback p99 rose from 33.91 to 37.23 ms.
Callback mode allocated 46.90 MiB per 30-second motion phase versus 3.92 MiB,
primarily from detached queued payloads. It remains experimental, not the
production default. These are historical isolated measurements, not today's
30-FPS live application CPU or battery savings.

[T403 direct compressed input](../benchmarks/2026-09-18-decoder-input.md)
removed the heap-to-codec staging copy in an experimental SocketChannel path.
On motion/60 FPS, heap/direct CPU was 50.05%/50.32% of one core and feed-to-output
release p99 was 34.54/34.68 ms. The separate replay includes both local socket
peers; it is not directly comparable to T386's CPU totals. No useful improvement
was established, and `VideoReceiver` retains the reusable heap reader.

[T404 timing lookup work](../benchmarks/2026-09-17-timing.md) retained the current
primitive-array cache. It improved delayed lookups in a host JVM replay while
slightly worsening immediate/colliding lookups. Those nanosecond-scale bookkeeping
results are not an Android frame-latency percentage.

Today's [T556 live sample](../benchmarks/2026-09-20-evdi-restart/README.md) has
tablet arrival-to-render-callback window p50 values around 12.3 ms at 30 FPS.
That interval combines multiple stages; it neither identifies CPU saturation
nor measures when pixels become visible. Historical SurfaceFlinger work also
demonstrated that [decoder callbacks do not certify physical presentation](../benchmarks/2026-09-19-presentation-power.md).

## Next evidence, before implementation

T560 keeps a narrow current-playback profiling follow-up open: measure per-thread
CPU/runnable/blocking time, codec input/output waits, frame queue occupancy,
allocation/GC and compositor timing together. [Android system tracing](https://developer.android.com/topic/performance/tracing)
can show scheduler and frame activity; method-level costs require a CPU profile.
Keep instrumentation overhead, missing counters and time boundaries explicit.
No activity replacement, screen lock, ADB reset or EVDI restart is needed merely
to assess passive observations.

Only pursue an extra worker, changed queue structure, bounded buffer pool or
platform-specific SIMD kernel when a measured cost justifies it. A callback
buffer pool is a candidate if callback mode is selected later, not evidence that
the production synchronous reader currently allocates each frame. Any change
must preserve access-unit order, codec ownership, cancellation and user capacity
choices, with permanent regressions and matched before/after measurements.
