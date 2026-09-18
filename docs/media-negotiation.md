# Media capability negotiation and selection research

T477, 2026-09-18. This document records the research and implementation contract
for T478/T479. The T478 report/selection contract is implemented; T479's measurement
and ranking gate below remains follow-up work. See
[video codecs](video-codecs.md) for the running protocol. Windows remains a
[planned host port](windows-port.md).

## What announcements can establish

The current Android `DecoderCapabilities` report identifies codec families and
whether a compatible decoder advertises hardware acceleration for the negotiated
width, height and FPS. `ControlSession` collects it on its IO scope, serializes
inventory queries and rejects completion after socket/format retirement. The
host separately probes stock encoders, ranks their output cadence and requires
three new render acknowledgements. T468 aligns decoder creation with the
format-compatible inventory; T465 monitors failures after initial verification.

Richer announcements can reject impossible combinations before a disruptive
trial. They cannot prove a faster decoder, correct vendor firmware, effective
hints, image quality, sustained throughput or lower power. Keep these distinct:

| Evidence | Meaning | Does not establish |
| --- | --- | --- |
| Advertised | Platform reports support for a format or feature | Successful configuration or performance |
| Requested | UScreen supplied a profile/hint to an adapter | Adapter accepted or applied it |
| Configured | Actual encoder output or decoder configuration was inspected | Correct sustained rendering or optical latency |
| Render verified | Fresh acknowledgements belong to this stream generation | Quality, optical presentation or battery improvement |
| Measured | Named workload, boundary, sample population and conditions | An untested workload/device/transport |

Useful inventory comprises decoder identity, codec, profile/level pairs, inferred
profile bit depth, exact format compatibility, acceleration classification and
standard low-latency/rate support. Identity is session-local, not a stable
cross-device cache key. Report unknown separately from false. Maximum instances
and advertised rate headroom are resource limits, not measured capacity.

Android's [capability API](https://developer.android.com/reference/android/media/MediaCodecInfo.CodecCapabilities)
checks decoder profiles on supported Android versions, but its format query does
not prove that stream parameters comply with the advertised level. Validate
profile/level pairs separately and retain configuration/render checks. Standard
low-latency support is available from API 30; older APIs are unknown, not proof
that decoding must be slow. The [codec guide](https://developer.android.com/media/optimize/performance/codec)
documents acceleration/identity APIs from API 29 and explains that performance
points may be unavailable after an OS upgrade.

Use codec-standard names on the wire, translating Android's
[profile/level constants](https://developer.android.com/reference/android/media/MediaCodecInfo.CodecProfileLevel)
inside the Android adapter. Unknown profiles must not acquire a guessed bit
depth or level ordering. Main10 support does not mean HDR support. The capture
path starts with eight-bit pixels; conversion to ten-bit HEVC cannot recover
discarded precision or produce HDR metadata.

The [MediaFormat contract](https://developer.android.com/reference/android/media/MediaFormat)
distinguishes requested encoder profiles from actual output. Operating rate and
priority are requests, not speed guarantees. Do not forward arbitrary vendor
keys; a private hint needs a documented vendor/codec applicability rule and a
separate experiment. Preserve legacy behavior for old peers until a validated
replacement is selected. The optional in-process encoder is a different adapter
and cannot inherit CLI probe evidence.

Tablet encoder inventory has no selection role: Android receives and decodes
host video; it does not encode it. Do not enlarge startup reports or rank the
receive path using tablet encoder availability. A future reverse-video feature
would need its own capability direction and contract.

## T478 contract

Extend the control protocol, retaining the existing family report for old peers.
The host explicitly advertises the richer version. New Android must not send a
new protocol to a host which did not request it. Missing/unknown versions keep
the existing H.264 and version-one paths; unknown detail cannot authorize a new
profile. A malformed richer report must not replace a working report.

Scope reports to the controlling connection, format revision, encoded width,
height and FPS. Refresh on reconnect and relevant format changes, including a
change away from and back to the same format. Serialize platform collection off
the UI/control locks, bound its result, and discard cancelled/stale completion.
Video startup continues on the working fallback while inventory is pending.

Use platform-neutral JSON with explicit bounds within the existing 64 KiB
WebSocket message limit: four families, at most 16 decoder entries, names at
most 128 bytes and at most 32 profile/level entries per decoder. Include exact
format support, canonical profile/level/depth, nullable acceleration and
nullable standard hint support. Missing, failed and unrecognized queries remain
unknown; truncation must never turn an incomplete list into exhaustive rejection
of the legacy fallback. No arbitrary keys, executable names or library paths
from a tablet may become host command arguments. The implemented format and
old-peer behavior are described in [video codecs](video-codecs.md#negotiation).

Selection intersects **actual stock encoder output**, the conversion adapter,
wire restrictions and one matching Android decoder. Inspect the probe bitstream
instead of assuming a wrapper's default profile. H.264/HEVC use Annex B;
VP9/AV1 retain bounded framed packets and their existing eight-bit 4:2:0
restrictions. Only the implemented HEVC path may request ten-bit conversion.
Use stock FFmpeg interfaces, never a patched FFmpeg or guessed bitstream edits.

A richer selection identifies the decoder, encoded format and requested standard
hints. Android validates them again against local capabilities before creation;
configuration failure or missing render progress advances to a bounded fallback.
Requested hints are not reported as effective settings. No default preference,
explicit override or persisted user setting is silently migrated. T417's later
Android diagnostic UI remains separate.

## Existing measured comparison

The [codec/profile experiment](benchmarks/2026-09-18-codecs.md) retains matched
fixtures, repeated physical replays, quality measurements, commands and hashes.
The [HEVC investigation](benchmarks/2026-09-18-hevc-interop.md) tests the
advertised-versus-working distinction. These are existing observations, not a
new benchmark of the proposed protocol.

| Candidate on RugKing Pad 2 Pro | Existing evidence | Selection implication |
| --- | --- | --- |
| VAAPI H.264 High | 60 FPS Android feed-to-release p50/p99 27.77/29.72 ms; sparse 5 FPS 211.81/213.42 ms | Family compatibility hides profile delay |
| VAAPI H.264 Constrained Baseline | Same experiment 11.03/12.68 ms and sparse 11.87/13.05 ms; identical decoded hashes across tested profiles; motion bytes +23.8% | Strong candidate for a matched profile trial; bandwidth tradeoff is real |
| libvpx VP9, hardware decoder | Motion feed-to-release p50/p99 10.60/16.00 ms in the separate codec cohort | Worth comparing; not proven faster than Baseline |
| libaom AV1, software decoder | Motion feed-to-release p50/p99 89.53/110.08 ms | Smaller encoded data does not imply a better UI |
| VAAPI HEVC, advertised hardware decoder | Native error 14; x265 control streams work | Profiles/levels alone cannot certify interoperability |

Host-only paced motion p50/p99 was 3.273/4.383 ms for VAAPI H.264 depth one,
1.586/2.293 ms for libx264, 3.198/6.048 ms for VP9 and 7.198/59.702 ms for AV1.
That boundary is raw write admission to encoded output, not display latency.
Do not sum its percentiles with Android replay percentiles. The host's live
packet-ready-to-render-ACK timer includes delivery and the return path but omits
capture/encoding. Render callbacks are not optical measurements.

There is no matched full-path comparison of the current automatic policy and
the proposed capability-informed policy. Existing short power snapshots and the
[interrupted sustained power matrix](benchmarks/2026-09-18-power-validation.md)
do not establish battery savings; the gauge advances in coarse 9.99 mAh steps.
Startup/recovery regressions establish lifecycle behavior, not physical timing.
The research therefore supports richer admission and further measurement, not
an unqualified performance claim or automatic promotion based on advertising.

## T479 measurement and ranking gate

Compare the current policy and compatible candidates on this tablet only. Use
matched motion, text/pen, sparse and recovery workloads, fixed dimensions/FPS,
transport, brightness, display refresh and quality floors. Alternate trial order;
retain raw data, actual commands/stream/APK hashes, invalidations and exclusions.
Keep decoder-only, host-only, render-ACK and actual end-to-end boundaries distinct.
Report p50/p95/p99, achieved FPS, startup/recovery distributions, image quality,
bytes and sustained whole-tablet charge/thermal observations. Missing evidence
stays unknown; app-process CPU cannot stand in for whole-device power.

Use a bounded candidate set and calibration deadline. Preserve manual choices,
cancel on foreground/idle guard failure or relevant host/device/format change,
and keep a working fallback. Reject stale results; do not cycle repeatedly among
failed candidates. First eliminate incompatibility, failed rendering, insufficient
throughput and unacceptable quality. Compare latency tails/startup within the
survivors; document bandwidth and sustained power tradeoffs rather than inventing
a universal weighted score. A winner is best tested under recorded conditions.
Do not attach/restart EVDI for measurement while T222 remains unresolved; use an
isolated path or retain the missing full-path evidence explicitly.

Cross-tablet persistent configuration and a multi-tablet campaign remain outside
this work (T480 deferred; T382 declined).

## Permanent automated coverage required before behavior changes

| Layer | Required regressions |
| --- | --- |
| Shared wire/policy | Valid bounded round trip; malformed/oversized/unknown versions; profile/level/depth/hint intersections; unsupported and unknown remain distinct |
| Android inventory | API 27/29/30+ behavior, exceptions, missing features, identity bounds, exact format/profile support and off-main collection |
| Control lifecycle | Old host/new tablet and new host/old tablet; reconnect, changed FPS/dimensions, away-and-back format, cancelled or late report cannot replace current state |
| Decoder selection | Selected decoder belongs to advertised compatible tuple; local revalidation; vendor hints not generically forwarded; failed configure/hints/watchdog fallback |
| Host adapter | Stock encoder's measured output agrees with selected profile/depth; capture/wire restrictions; unavailable encoder/probe failure retains fallback |
| Ranking/recovery | Deterministic tradeoffs and evidence identity; bounded calibration; manual/background/idle cancellation; failed candidate tried once; no stale result or rollback promotion |

Behavioral fixes require the permanent regression to fail before the fix and
pass afterward. Physical performance claims additionally require the named
measurements; unit tests alone cannot establish them.
