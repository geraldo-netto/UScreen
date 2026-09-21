# T575: initial GPU capture and VAAPI import feasibility

Historical preliminary probes, superseded by the
[implemented prototype and patterned physical USB measurements](2026-09-21-gpu-adapter/README.md).
Those later checks found cross-device corruption missed by the uniform-color
probe below. The prototype rejects cross-device import and remains opt-in
because measured latency worsened.

An alternative capture adapter can plausibly remove UScreen's CPU framebuffer
read, RGB-to-NV12 conversion, raw FIFO transfer and hardware upload. Native
DRI3 export and VAAPI import succeed on this machine. This is feasibility
evidence; these initial probes did not implement a streaming backend or measure
a performance gain.

The portable-host refactor T523 (`5afa47b`) completed before these probes.
The live Linux/Android stream continued; no service restart, display mode
change, desktop-session switch, privilege change or Android power action was
performed. Xorg PID 2512 and live FFmpeg PID 1037128 remained present.

## Current path

The live FFmpeg 6.1.6 process reads NV12 at 1280×800, 30 FPS from
`capture.fifo`, then applies `format=nv12,hwupload` before H.264 VAAPI encoding
on `/dev/dri/renderD128`. The source agrees:

- `host/evdi/capture.c::on_mode_changed` registers an allocated CPU framebuffer;
  `grab_now` calls `evdi_grab_pixels`, followed by CPU conversion/publication.
- `host/src/capture/cli_encoder.rs` selects rawvideo FIFO input and hardware
  upload for VAAPI.
- T418's leased shared input applies to the optional in-process path, with
  automatic selection enabled only for measured libx264 input. It does not
  remove the current VAAPI input copies.

Stock EVDI's public interface fills the caller's registered memory. It does not
return an encoder-ready GPU frame through that interface. A new capture route
must bypass that read path; changing the encoder name or replacing the FIFO
alone does not establish zero-copy capture. See the
[EVDI buffer/update contract](https://displaylink.github.io/evdi/details/).

## Native checks

Environment: Cinnamon on X11; AMD Radeon 610M drives the primary X screen
(`/dev/dri/renderD129`), while current encoding uses Radeon RX 6600 XT
(`/dev/dri/renderD128`). Both use Mesa 26.2.3 radeonsi on kernel 7.0.0-31.
Node numbers are observations, not identifiers to hard-code in an adapter.

| Route/check | Observed result | What it establishes |
|---|---|---|
| ScreenCast portal | `org.freedesktop.portal.ScreenCast` interface absent | PipeWire portal capture is unavailable in this running session. It says nothing about every Cinnamon version/session. |
| Stock bundled FFmpeg `kmsgrab` | EVDI and AMD scanout metadata readable, but no framebuffer handle returned | Normal-user KMS capture cannot proceed here. The documented route requires DRM master or `CAP_SYS_ADMIN`; no privileges were granted. |
| Bundled FFmpeg VAAPI/DRM mapping | Eight synthetic BGR0 frames converted to NV12 and encoded as constrained-baseline H.264 on each GPU | The bundled build supports that mapping/filter/encoder combination. This round trip begins with CPU test-pattern upload and may reuse the originating surface; it is not independent external-import proof. |
| X11 DRI3 version | Negotiates 1.0; 1.2 requirement rejected | Current route lacks explicit modifier negotiation from DRI3 1.2. Extension presence alone was insufficient. |
| DRI3 synthetic pixmap export | 1280×800, 32 bits/pixel, depth 24, stride 5120, one 4,096,000-byte DMA-BUF | An independently created X pixmap can be exported by the normal user. Modifier is implicit/unknown, not asserted to be linear. |
| External VAAPI import | `vaCreateSurfaces` with `DRM_PRIME_2` succeeds on both GPUs | The exported X buffer can enter both native VAAPI devices. This includes the current cross-GPU arrangement. |
| Synthetic pixel verification | X11 RGB `55aa33` matches the downloaded VAAPI image on both GPUs | Import preserves the checked synthetic pixel; success is not inferred solely from API status. Verification intentionally downloads pixels. |
| Tablet-region source | X11 copy of the existing 1280×800 region at `(1920,0)` into a private pixmap exports/imports on both GPUs | Capture-source plumbing is available without a new window or mode change. No desktop pixels were saved; visual completeness, cursor behavior and sustained streaming were not validated. |

The first mapping command omitted the DRM parent device and failed device
derivation. Initial verification through a derived-image mapping read zero;
an explicit verification image plus `vaGetImage` returned the expected pixel.
Both unsuccessful probes are retained. Neither is diagnosed as a production
UScreen defect or evidence that the eventual capture path must download frames.

The [probe source and logs](2026-09-21-gpu-capture.tar.gz) retain commands,
environment, metadata, successful and unsuccessful results. The C probe is a
short-lived research executable with bounded subprocess timeouts, not a
production resource-lifetime implementation. Development headers were downloaded
and extracted under `/tmp`; no system package or bundled library was modified.

## Experiment proposed at the time

Build an opt-in Linux `CaptureBackend` adapter that keeps native GPU handles
inside the adapter and preserves the shared encoded-packet/session contract:

1. Capture the owned tablet region into a bounded set of X11 pixmaps and export
   them through DRI3. Observe damage where available, handle cursor composition,
   crop/rotation and geometry changes, and verify the actual captured picture.
2. Import the exact format, stride, offset and modifier representation into
   VAAPI, convert RGB to NV12 on the GPU, and encode through stock FFmpeg APIs.
   Prefer testing the desktop GPU as well as the currently selected encoder GPU;
   a cross-device import can entail transfers even when import succeeds.
3. Define producer completion and consumer release explicitly. Do not reuse a
   pixmap before GPU reads finish; tie descriptor/frame lifetime to final
   references and handle cancellation, resize, import failure and driver loss.
   Do not infer synchronization from successful buffer allocation or add sleeps.
4. Compare matched workloads against current EVDI/FIFO VAAPI using complete
   capture-to-encoded-packet and capture-to-render-ACK measurements, CPU/GPU
   cost, frame correctness and delivered-frame coverage. Retain FIFO fallback
   and user choice until this path proves consistently better.

Keeping EVDI for virtual-monitor ownership may still incur Xorg/EVDI output
copies even when UScreen captures elsewhere. X11 copying and RGB conversion
also write GPU surfaces. The defensible target is **avoiding application CPU
readback/upload**, with measured latency benefit. Neither zero memory traffic
nor full-pipeline zero-copy through ADB/Android follows from these probes.
The subsequent T575 experiment is implemented and documented in the linked
report above. FIFO remains the default.

API references: [FFmpeg KMS capture](https://ffmpeg.org/ffmpeg-devices.html#kmsgrab),
[FFmpeg 6.1 VAAPI mapping implementation](https://ffmpeg.org/doxygen/6.1/hwcontext__vaapi_8c_source.html),
[XCB DRI3 API](https://xcb.freedesktop.org/manual/group__XCB__DRI3__API.html),
[libva external DMA-BUF descriptor contract](https://github.com/intel/libva/blob/2.20.0/va/va_drmcommon.h),
and [PipeWire modifier/lifetime negotiation](https://docs.pipewire.org/devel/page_dma_buf.html).
