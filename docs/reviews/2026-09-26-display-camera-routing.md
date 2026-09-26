# T553: simultaneous display and camera route assessment

Keep current route selection. This narrow sample showed no sustained local TCP
backlog or measured benefit that warrants automatic balancing. It does not prove
capacity for every workload. No alternative paired network route was available.

## Scope and ownership

On 2026-09-26 the RugKing Pad2Pro used one authorized USB ADB endpoint negotiated
at 480 Mb/s. Display was 1280×800 at a configured 60 FPS, with T612's one-worker
libx264 fallback. The user explicitly approved one 30-second front-camera test
alongside display sharing. The existing production decoder probe requested
3 Mb/s, adaptive freshness 150 ms, with no forced stall. Only counters and timing
were retained; no images. This predates the T627 decoder-selection fix and must
not be interpreted as measuring that later revision.

`host/src/camera/bridge.rs` selects one ADB serial and owns a private reverse
mapping to a loopback listener. `host/src/linux_main.rs` prefers USB for display
and deduplicates proven physical identity. Independent serial selection is not
link bonding or a direct network-camera backend. A future split-route trial
needs a paired endpoint proven to be the same tablet, explicit per-resource
policy and preserved permission, lease ownership and reconnect behavior. Native
transport/process/mapping details belong inside backend adapters.

## Observations

The display-only and combined windows have different, uncontrolled desktop
content. Their different frame rates are not an A/B speedup. Display timing
starts at packet readiness and ends at its accepted render ACK, including ACKs
just after the sampling window; it is not optical presentation latency.

| Observation | Display only | Display + front camera |
| --- | ---: | ---: |
| Sample duration | 21.736 s | 28.381 s |
| Encoded display packets / accepted receipts | 846 / 846 | 1,688 / 1,688 |
| Display receipt rate | 38.92 FPS | 59.48 FPS |
| Display ACK p50 / p95 / p99 | 16.041 / 17.504 / 21.057 ms | 11.228 / 16.682 / 20.552 ms |
| Display TCP payload acknowledged | 0.0311 Mb/s | 0.0422 Mb/s |
| Camera TCP payload received | — | 2.9931 Mb/s |
| Camera feedback TCP payload | — | 0.0019 Mb/s |
| Maximum sampled display / camera Send-Q | 0 B / — | 0 B / 8 B |
| ADB CPU time | 0.15 s | 0.37 s |
| Display encoder CPU time | 1.38 s | 2.84 s |
| Capture helper CPU time | 1.47 s | 3.02 s |

Camera decoding produced **880 frames in 30 s (29.33 FPS)**, first frame at
584 ms and longest subsequent decoded-frame gap 44 ms. Its decoder used 2.42
CPU-seconds in the combined sampling window, with sampled peak RSS 71.0 MB.
Display encoder sampled peak RSS stayed 86.2 MB; ADB reached 8.8 MB. Process
resource deltas check process identity. Raw samples preserve all counters.

These are local TCP staging counters, not USB bus utilization. One-second
sampling cannot rule out short queues. Display payload was especially low;
this sample cannot substitute for historical bulk-transfer or high-motion tests.
Codec work is counted separately from routing, but there was no competing route
for causal comparison. Camera timing ends at decoder output, without V4L2 or
webcam presentation; T621 retains that missing native acceptance. No ADB reset,
Android lock, EVDI detach/fault experiment or broad T382 campaign was performed.

## Decision and cleanup

No automatic policy is justified by these observations. Reconsider with a
specific congested workload and an identified paired alternative route as new
scoped work. This decision does not reopen deferred broad campaigns.

Android records sensor connect at 17:44:13 and disconnect at 17:44:43 Europe/Rome.
Readback shows no active camera clients. The probe removed its private reverse
route while display/input mappings remained. Later T627 display-only validation
confirmed the camera had not reopened. The coordinator's `camera_finished_at`
includes waiting for its sampler and is not the sensor-stop time; the summary
excludes samples after camera TCP closure. Temporary host processes were later
stopped normally. No production behavior changed; no artificial test was added.

[Raw observations and reproducible summary](artifacts/2026-09-26-task-batch/t553/)
use the [T612 host trace](artifacts/2026-09-26-task-batch/t612/native-auto.log.gz).
The existing `host/examples/camera-feedback-probe.rs` requires capture
authorization. Passive observation scripts here are research artifacts.
