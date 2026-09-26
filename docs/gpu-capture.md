# Experimental Linux X11 GPU capture

T575 adds an explicitly enabled prototype for X11 and H.264 VAAPI Baseline.
**FIFO remains the default.** On the tested machine, GPU capture reduced the
measured processes' CPU use but increased video latency. See the
[measurements and limitations](benchmarks/2026-09-21-gpu-adapter/README.md).

## Build and enable

Build the ordinary host without `inproc-encoder`. Separately build the native
helper against an unmodified FFmpeg SDK and the native development packages
listed in `Makefile`'s `GPU_PACKAGES`:

```sh
make build-gpu-helper
make test-gpu-helper
```

`PKG_CONFIG_PATH` selects the SDK. `GPU_CFLAGS` and `GPU_LIBS` can select explicit
include/library paths. The validated SDK is stock FFmpeg **6.1.6**, matching the
bundled release version. Ordinary builds and AppImages do not acquire this
experimental helper or its development dependencies automatically. A manually
built helper must be able to locate its runtime codec libraries; do not mix
arbitrary libraries into the production AppImage.

Set `BLENT_X11_GPU_HELPER=/absolute/path/to/blent-gpu-capture` in the
environment of a host process built with this change. The variable must reach
the daemon, not just the GUI. The value is an executable path, with no shell
expansion or command arguments. Remove the variable and restart that host
process to disable the prototype.

The helper defaults to its original periodic cadence. To test event-triggered
capture, also set `BLENT_GPU_CADENCE=damage` in that host process's environment.
`periodic` restores the comparison mode; invalid values fail to FIFO. Damage
mode needs XDamage/XFixes development and runtime libraries (`xdamage` is now
in `GPU_PACKAGES`). It coalesces damage inside the owned output, bounds each
queue-drain pass, polls cursor movement at most every 8 ms, and refreshes idle
pictures at 5 FPS (or the configured FPS if lower). It requests an intra picture
after 0.9 seconds, evaluated at the next capture. Pattern fixtures stay periodic.
Neither variable changes the default application capture path.

The existing settings select the encoder (`h264_vaapi_baseline`), render node,
dimensions, scale, frame rate, QP and maximum bitrate parameter. Disable
`adaptive_idle` and `ten_bit` for this prototype. Like the existing VAAPI CQP
profile, forwarding the maximum bitrate parameter does not promise a hard
network bitrate cap. GPU scaling uses the driver's video processor; it is not
bit-identical to the CPU box filter.

The selected render node must be the **same DRM render device used by X11**.
The helper verifies device identity. Node numbers vary between boots and
machines; it does not silently change the user's GPU preference. Cross-device
implicit-layout imports produced corrupted patterned frames on the tested
hardware despite successful API calls, so they are rejected before publishing
a stream header.

## Ownership and fallback

The portable capture/session interfaces still exchange encoded packets.
All X11, DRM and VAAPI handles stay inside the Linux adapter process.

The host passes the owned EVDI connector's EDID. Native code requires a unique
matching RandR output; sysfs and Xorg connector names need not agree. It checks
the output's identity, CRTC, geometry and transform before and after capture.
Moved, disconnected, rotated or transformed outputs end this adapter instead
of capturing another region. The ordinary capture path remains available.

One private XRGB8888 pixmap is exported through DRI3. Its modifier remains
implicit/unknown. XSync fences establish producer completion before VAAPI reads
it. GPU video processing converts it to an independently owned NV12 surface;
`vaSyncSurface` completes that final RGB read before the pixmap is reused.
Libavcodec retains each NV12 surface through its final frame reference.
Cursor pixels are composited using XFixes/XRender and can pass through CPU
memory. The desktop pixel path performs GPU copies and conversion.

Startup has a five-second native deadline; each frame's native waits and
output have a two-second deadline. Child cancellation terminates the process.
Native errors or EOF disable further GPU attempts for that capture owner and
resume FIFO encoding without detaching the EVDI monitor. The FIFO reader is
retired and its inode rotated before reuse. A new capture owner can try the
explicit opt-in again.

The retained Linux EVDI owner converts BGRA to NV12 only while its FIFO has a
reader. GPU capture leaves that FIFO unread, so conversion workers stay asleep;
EVDI continues requesting updates, grabbing pixels and acknowledging flips.
On FIFO fallback, opening the reader wakes the capture loop through an eventfd
and forces a complete conversion of the current framebuffer, even on a static
desktop. Mode/reader generation checks discard conversions that cross a
transition. The writer retains its existing 50 ms reader-discovery interval;
fallback needs no new screen damage or EVDI detach. A failed eventfd allocation
preserves continuous conversion and logs that fallback. The shared-memory raw-ring
transport keeps its separate slot ownership and damage handling.

This is **not full-pipeline zero-copy**. Stock EVDI retains monitor ownership
and continues CPU readback; Xorg/EVDI and GPU conversion
still copy pixels, and USB/Android have their own buffers. T579 added the
opt-in damage mode and [matched native measurements](reviews/2026-09-26-next-batch.md#t579--damage-triggered-gpu-capture).
It improves the measured 29 Hz case but worsens p95 against periodic capture at
30 Hz. T593 tracks that tail latency; T578 tracks explicit cross-device layout negotiation. [T580 measurements](benchmarks/2026-09-21-fifo-demand/README.md)
cover the redundant conversion removed from the retained EVDI owner.

## Repeatable validation

The normal Rust suite includes T575 eligibility, settings, ownership,
cancellation, fallback, native argument bounds/fuzzing and actual encoded
packet compatibility tests. `make test-gpu-helper` additionally verifies cursor
cropping, codec error handling, damage/idle deadlines, cursor-only wakeups,
foreign-region filtering and lease rejection in an isolated Xvfb server.
The normal EVDI helper suite also retains T580 regressions for absent/early
readers, static-screen reconnect, resize, stale generations, immutable leases,
generation wrap, eventfd failure and concurrent reader transitions. These run
without a kernel EVDI device, with address/undefined/thread sanitizers.

For a real GPU, `scripts/benchmarks/verify-gpu-capture.py --help` describes the
explicit unused EVDI card/output fixture. It checks patterned colors at scales
1–4, cross-device rejection, bounded blocked output, layout-change rejection
and fresh capture after retirement. It changes only that temporary test output
and requires a same-device and different-device render node.

`gpu-capture.py` and `summarize-gpu-capture.py` under the same directory perform
the separate physical USB replay. This temporarily uses the benchmark Android
activity and restores Blent afterward. Supply an unused EVDI output and a
matching replay APK; the currently documented decoder choice is specific to
the measured tablet. Compare decoded source identities and complete ACKs, not
just frame counts. A 29 Hz source with 30 FPS capture reduces equal-cadence
phase bias. Neither script measures optical presentation.
