# T561: six queued buffers without a latch

All six unlatched interior buffers in the retained 1280×800/30 FPS trace have
a newer buffer queued before the next observed latch. The successor is latched
and has a present fence. The pattern is consistent with compositor supersession
of closely spaced updates. It does not prove where the input burst began, and
it is not evidence that six compressed packets were lost.

| Surface frame | Next queue after it (ms) | Successor latch after it (ms) | Nearby codec input gap (ms) | Codec release gap (ms) |
| --- | ---: | ---: | ---: | ---: |
| 149083 | 4.987 | 8.045 | 5.744 | 5.341 |
| 149095 | 16.396 | 17.753 | 16.539 | 16.557 |
| 149239 | 5.431 | 15.732 | 5.433 | 5.490 |
| 149260 | 5.587 | 10.645 | 3.862 | 5.007 |
| 149449 | 15.288 | 17.577 | 14.078 | 15.087 |
| 149758 | 13.749 | 14.564 | 14.259 | 13.626 |

The input pairs are consecutive wire-sequence timestamps temporally adjacent
to each surface event, retained in [correlation.json](correlation.json). The
trace does not directly attach a UScreen sequence to a SurfaceFlinger frame
number, so this is temporal correlation, not a claimed explicit cross-layer
identity join. Input bursts already span 3.862–16.539 ms before output is
released; the output release calls themselves last only 7.962–10.692 µs in
these pairs. Adding output threads would not remove those upstream arrival
gaps. The existing T560 runnable/GC observations remain supporting evidence,
not a proof that every scheduling delay is absent.

`DecodedOutputDrainer.present` releases decoded output with `render=true` as
soon as it is ready. The default profile does not enable the experimental
render-latest discard loop. Both buffers here reached SurfaceFlinger's Queue
stage; an application-side discard before queueing cannot explain that pair.
A render callback/ACK is distinct from a SurfaceFlinger latch or present fence,
and a present fence is still not an optical photon measurement.

The retained host journal has five-second aggregate latency reports, not the
per-sequence encoded-ready/capture/transport timestamps needed to identify the
burst's origin. The video wire uses sequence identity in Android's timestamp
field, not the original capture timestamp. Consequently this historical trace
cannot distinguish capture pacing, encoder delivery, USB/socket scheduling or
receiver/input scheduling as the root cause. T561 remains unresolved on that
specific missing correlated evidence; no speculative pacing or frame-drop
change was made.

## Optional host diagnostics

The CLI encoder now emits `Packet ready` events on the dedicated
`uscreen::frame_timing` TRACE target: encoder epoch, sequence, media timestamp
when available, packet bytes, keyframe flag and monotonic microseconds since
that encoder stdout drain began. `Render ACK received` events include epoch,
sequence and packet-ready-to-ACK duration, only after receipt validation and
sequence lookup. Duplicate or unmatched ACKs produce no timing event.
No payload, token or camera contents are logged.

Default `uscreen=info` logging remains unchanged. At a safe future normal
startup, enable only this diagnostic target with:

```sh
RUST_LOG='uscreen=info,uscreen::frame_timing=trace'
```

Set that environment on the host daemon using the usual launch mechanism;
it is not a command to restart or detach the current display. Collect a bounded
host journal window alongside the existing Android Perfetto configuration.
Join packet/ACK events by encoder epoch and sequence, and locate codec input
sequence labels in Android. Compare *within-clock* gaps; do not subtract Linux
and Android timestamps without clock alignment. These logs cover encoded-ready
delivery and callback ACKs; a capture/FIFO probe is still needed if encoded-ready
bursts point further upstream. Trace logging itself can perturb timing, so
retain the enabled filter and compare with the ordinary passive workload.

The permanent T561 regression first failed with zero per-packet events and now
requires both packet records, their identities/timestamps and exactly one
accepted ACK despite a wrong-decoder and duplicate ACK. Existing framing and
lifecycle regressions remain intact. This fixes the diagnostic gap, not the
still-unattributed historical omissions.

## Reproduction evidence

The original trace remains at the path/hash in [provenance.json](provenance.json).
Run the retained SQL with Perfetto trace processor against that trace. Compressed
CSV extracts retain all selected codec calls plus the first two neighboring
frame windows and detailed slices for frame 149083. The six Queue times and
successor latch/present values can also be recomputed from T560's committed
`frame_events.csv.gz`, without access to a live tablet. T560's retained quality
query reports no nonzero trace errors/data-loss counters; missing counters
cannot independently guarantee a complete trace.
