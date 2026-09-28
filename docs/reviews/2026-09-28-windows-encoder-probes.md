# T530 isolated Windows encoder tooling

`host::encoder_probe` enumerates stock FFmpeg video encoders, builds the T685
recipes and probes generated frames without capture or display creation. Native
program paths are injected. Shared command owners provide Linux process groups
and Windows kill-on-close jobs; cancellation drops the owner. Each operation has
an eight-second deadline, with 45 seconds total for selection and a one-MiB
inventory limit. Probes request exactly 65 frames and retain one independent
sample, using existing bounded/checksummed packet framing and Annex B assembly.

Results distinguish advertised names, observed encoded initialization, packet
count, format inspection, compatible tablet decoder and failure. A launch or
encoder listing is not successful initialization. The common ffprobe parser
checks codec, profile, depth, level and exact dimensions; HEVC high tier is
rejected. Protocol-v2 decoder capabilities are required. Explicit choices probe
only that encoder, so failure is visible. Automatic mode tries software first,
including shared 1/2/4 worker choices, then hardware recipes. Both platforms use
the same ranking: meet requested FPS, prefer a compatible hardware tablet decoder,
then lower p95 packet interval, first-packet delay and stable encoder-name order.

First-packet delay includes process startup. Steady throughput and p95 interval
exclude the first eight packets; these are generated-frame, host-observed packet
measurements, not capture-to-tablet or optical latency. Requested worker count
stays separate from effective x264 SEI evidence. No GPU usage or transfer cost is
inferred. A changed session epoch, decoder advertisement or requested setting
invalidates the selection. Probe output never enables the production pipe adapter.

Run development tooling with:

```
cargo run -p blent --example probe_encoders -- tablet-capabilities-v2.json
cargo run -p blent --example probe_encoders -- tablet-capabilities-v2.json libx264
```

Use an actual bounded decoder advertisement for meaningful compatibility. The
repository fixture is useful for isolated tests only. T529 owns streaming
integration; T677 owns actual GPU/tablet encode/decode/render acceptance and
candidate enablement. Physical support remains unavailable until those gates.
