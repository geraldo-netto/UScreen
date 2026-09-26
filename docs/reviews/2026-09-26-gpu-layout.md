# T578: explicit GPU layout prerequisite

The native DRI3 query requested version 1.2 and negotiated **1.0** on this X11
session. The experimental explicit-layout path therefore stopped before import
or encoded output. [Probe output and identity](artifacts/2026-09-26-performance/t578/)
are retained. A development-only copy attempted `BuffersFromPixmap`, with its
modifier, plane offset, stride and dma-buf size; no production admission guard
was changed.

DRI3 1.2 adds the multi-buffer/modifier export needed by that route. The
[protocol specification](https://sources.debian.org/src/xorgproto/2018.4-4/dri3proto.txt/)
describes the version boundary. Existing `BufferFromPixmap` in
`host/gpu/capture.c` exposes the implicit layout; `encode.c` correctly labels it
`DRM_FORMAT_MOD_INVALID`, which is not a claim of linear layout.

Earlier patterned testing already established corrupted output when importing
Raphael/X11 memory into Navi23, despite successful surface import. The current
same-render-node check in `identity.c` remains necessary. Uniform-color tests
or a successful `vaCreateSurfaces` call cannot establish pixel correctness.

T578 stays blocked until either the actual X11 graphics stack supports validated
explicit export or a separate owned linear/exportable surface route is designed.
An extension upgrade alone would not complete it: modifier compatibility,
plane bounds, producer/consumer completion, patterned decode at supported
scales and complete USB render acknowledgements still require native validation.
Do not change the graphics stack during this application profiling session.

This result also bounds zero-copy work: same-GPU RGB import and GPU conversion
are already possible; safe cross-GPU import is not established on this stack.
FIFO remains the supported fallback. Raw probe source, executable and command
records are retained under the private performance archive documented in the
[main audit](2026-09-26-performance-audit.md).
