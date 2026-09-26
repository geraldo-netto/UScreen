# T599: display ownership is not independent of grabbing

Investigation complete. Do not suppress EVDI grabbing while the same-GPU consumer
runs. There is no supported ownership-only operation in the reviewed EVDI ABI.
This closes the proposed userspace shortcut without changing capture behavior.
T593's rejected cadence candidate does not prevent this independent ABI review.

## Evidence

`host/evdi/capture.c::on_update_ready` calls `grab_now` before requesting another
update. Its existing comment explains that grabbing completes compositor flips.
`publish_frame` already checks active frame-exchange generation before converting
BGRA to NV12. Thus the no-reader path already avoids conversion, while continuing
the EVDI update/grab lifecycle. The earlier FIFO CPU profile is not a measurement
of redundant conversion during GPU capture.

In pinned EVDI revision `2713cd41932f2bd8697953a205862a68a966b5ba`,
`evdi_painter_grabpix_ioctl` takes the pending CRTC/vblank, copies pixels, then
sends the vblank event. The public grab mode is DIRTY; zero rectangle capacity
is rejected. Request-update alone checks/arms damage and does not perform this
completion. Neither replacing `poll` nor choosing a newer syscall interface
changes that driver contract. See the pinned
[painter implementation](https://github.com/DisplayLink/evdi/blob/2713cd41932f2bd8697953a205862a68a966b5ba/module/evdi_painter.c#L1000-L1158)
and [ABI](https://github.com/DisplayLink/evdi/blob/2713cd41932f2bd8697953a205862a68a966b5ba/module/evdi_drm.h).

The installed Ubuntu `7.0.0-31-generic` module was also inspected directly:
`evdi_painter_grabpix_ioctl` has relocations to `_copy_to_user` and
`drm_crtc_send_vblank_event`. Its exact module hash and disassembly are retained
in [the evidence directory](artifacts/2026-09-26-followup/t599/). The distro module
is not asserted to be byte/source-identical to the pinned upstream revision.
This supports retaining the existing handshake; no intentional compositor stall
or invalid-ioctl experiment was necessary.

## Consequences

- An error-path trick using an invalid destination/dimension is not a supported
  copy-free acknowledgement contract and must not become a production adapter.
- A future driver extension would need explicit discard/acknowledge semantics,
  frame/fence ownership, cursor handling and full refresh on FIFO fallback.
  It would also introduce kernel deployment/version support obligations.
- A different virtual-display backend could avoid this dependency, but needs its
  own compositor/native validation. A shared interface alone is not support.
- Same-GPU import still avoids its own userspace raw-frame transfer. EVDI drain,
  GPU color conversion, encoder access and compressed transport prevent an
  end-to-end zero-copy claim. Cross-GPU explicit-layout admission remains T578.

No readback savings are claimed, no production code changed, and existing
ownership/fallback/restart/resize coverage stays intact. Before any future
suppression implementation, add permanent red-before-green regressions for
those lifecycle transitions and stale-frame rejection, then measure CPU and
complete render ACKs. The current ABI review resolves T599; it does not silently
approve maintaining a new kernel ABI.
