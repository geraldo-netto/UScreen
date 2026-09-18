# CLI input and access-unit assembly — 2026-09-17

T407 reduces copying in the default stock-FFmpeg CLI path and rejects oversized
unfinished NALs, access units and codec configuration before publication. It
preserves Annex B bytes, codec-header revisions, sequence identity and encoder
generation retirement. No FFmpeg patch or installed application change is involved.

## Implementation and bounds

The packetizer reads stdout into its own Vec's spare capacity through safe
`AsyncReadExt::read_buf` and a bounded `BufMut` view. Reads use at most 512 KiB;
small reads reuse the available capacity rather than requiring another full
512 KiB reservation. There is no initialized scratch array or subsequent
scratch-to-input copy.

The incremental T384 scanner remains. Consumed prefixes stay behind an offset
until reclamation is useful. Large consumed spans are reclaimed while the tail
is small; a small consumed prefix is not shifted just before an inevitable
allocation growth. This avoids both per-chunk front shifts and needlessly
moving most of the next large picture.

NAL payloads copy into one contiguous access-unit Vec. Keyframe configuration
is inserted before the first key slice, so publication does not copy the large
picture a second time. Existing prefix metadata stays after the prepended
configuration, preserving the previous bytes. A short pre-slice metadata prefix
may still be copied twice; allocator-internal moves during growth are also
possible. Unchanged normalized parameter sets retain their existing storage.
Small aligned headroom avoids doubling an allocation merely to append a tiny
continuation slice. Completed output owns its full Vec capacity through
`MediaBytes`; a slow consumer cannot keep a mutable parser buffer alive.

Shared immutable input segments/BytesMut slices were considered as an alternative.
They would need whole-backing identities across split views, a cap on segment
metadata and coalescing for the existing contiguous packet interface. A
seven-byte fragmented NAL could otherwise create a reference per tiny segment.
The implemented path removes the scratch copy while retaining simple contiguous
ownership. No measured claim is made for an unimplemented segmented/scatter path.
T416 records the separate opportunity to cap exceptionally dense output batches.

Bounds match the existing wire limits:

- A completed NAL and an assembled frame, including any prepended configuration,
  may contain at most 8 MiB minus four bytes.
- The retained unfinished NAL may temporarily include three extra bytes that
  could become the next split start code. EOF validates the completed size.
- Input allocation is capped at the frame limit plus one 512 KiB read and three
  lookahead bytes. Pending access-unit capacity is capped at the frame limit.
- Normalized parameter sets together and the immutable combined configuration
  each have an 8 MiB limit. Replacement size is checked before allocating the
  new set; old configuration remains immutable for queued consumers.

Oversized input returns an assembly error to the existing capture supervisor;
dropping the packetizer retires its generation. It does not send a partial frame
or silently continue with truncated codec headers. These staging caps complement
T391's 32 MiB admitted-backing budget. They are **not a total RSS bound**: parser
staging, temporary replacement allocations, output-batch metadata, allocator
bookkeeping and FFmpeg internal memory are separate. Whole Vec capacity, rather
than visible length, is charged when a frame is admitted.

## Measured result

| Codec / workload | Sessions | T384 CPU ms | Before T407 CPU ms | T407 CPU ms | Change vs before |
| --- | ---: | ---: | ---: | ---: | ---: |
| h264 / fragmented | 1 | 2.510 | 2.396 | 2.186 | -8.8% |
| h264 / fragmented | 2 | 4.861 | 4.805 | 4.406 | -8.3% |
| h264 / fragmented | 4 | 9.485 | 9.919 | 8.940 | -9.9% |
| h264 / dense | 1 | 8.567 | 9.212 | 8.005 | -13.1% |
| h264 / dense | 2 | 16.821 | 17.951 | 15.908 | -11.4% |
| h264 / dense | 4 | 33.517 | 36.120 | 31.983 | -11.5% |
| h264 / large_nal | 1 | 49.239 | 49.586 | 31.714 | -36.0% |
| h264 / large_nal | 2 | 100.433 | 99.145 | 62.763 | -36.7% |
| h264 / large_nal | 4 | 200.738 | 200.366 | 124.895 | -37.7% |
| hevc / fragmented | 1 | 2.356 | 2.363 | 2.257 | -4.5% |
| hevc / fragmented | 2 | 4.629 | 4.862 | 4.476 | -7.9% |
| hevc / fragmented | 4 | 9.239 | 9.708 | 8.979 | -7.5% |
| hevc / dense | 1 | 8.344 | 8.934 | 8.064 | -9.7% |
| hevc / dense | 2 | 16.790 | 17.982 | 15.833 | -12.0% |
| hevc / dense | 4 | 33.662 | 35.862 | 32.050 | -10.6% |
| hevc / large_nal | 1 | 49.709 | 48.631 | 31.251 | -35.7% |
| hevc / large_nal | 2 | 99.081 | 98.644 | 62.089 | -37.1% |
| hevc / large_nal | 4 | 200.625 | 200.286 | 124.245 | -38.0% |

These are medians of ten alternating runs per case, summed thread CPU time
across the listed sessions. Each session does the same amount of work, so the
multi-session totals are aggregate CPU cost, not a per-session latency or FPS.
The original committed T384 implementation (`1e7b045`) is shown explicitly.
The immediate pre-T407 baseline (`8305f04`) also includes T391's backing-budget
metadata and isolates the effect of this item. No pre-T384 baseline is reused.

Relative to the immediate baseline, this measured stage used about 4–13% less
CPU in the fragmented/dense cases and 36–38% less in the large-NAL cases.
This includes the bounded-input checks. Repeat ranges remain in the raw summary;
these are workload medians, not guarantees for every individual run.

For the single-session H.264 read replay, explicit-copy bytes fell from
1,381,680 to 654,900 for fragmented input, from 18,223,600 to 8,678,000 for
dense input, and from 105,026,620 to 42,028,700 for large NALs. Scanner spans
remained unchanged. The large-NAL replay's requested allocation bytes fell from
241,129,104 to 115,544,264 and reallocations from 280 to 60. Both sides include
runtime/parser setup; these are summed requests, not simultaneous live storage.

The original T384 `push` replay is retained separately. It deliberately copies
caller slices into the parser and therefore does not measure the production
scratch-copy removal. Its fragment-copy count can increase when offsets defer
reclamation, despite fewer reallocations; use the actual read-boundary replay
to assess the production path. Scanner input spans are unchanged. Neither
requested allocation bytes nor explicit-copy counters measure memory-bus traffic
or peak RSS; counters omit allocator-internal relocation.

These results establish local parser-stage costs only. Encoding, USB/ADB,
Android decoding, Surface presentation and battery use are outside timing.
Small differences on this shared workstation need broader validation; no
end-to-end latency or battery gain is claimed. The broader physical comparison
proposed under T382 is now closed as `wont_fix` for the current scope.

## Validation and method

Nine permanent T407 tests cover unfinished NALs, prefix-only access units,
combined CSD, enlarged IDRs, the exact frame limit and one-byte overflow,
read-copy elimination, bounded backing retention with delayed slices,
cancellation during a partial next NAL, and real H.264/HEVC decode equivalence.
The five initial limit/copy assertions failed on the pre-change implementation,
then passed unchanged. Existing T079/T080/T285/T384 prefix, partial-header,
configuration-revision and fragmentation assertions remain in the normal suite.

Real libx264/libx265 fixtures encode twelve 64×48 pictures through stock FFmpeg.
The parser reads them in 1/3/7/4093-byte fragments; output bytes, packet count,
sequence/keyframe/configuration identity and retirement match the unfragmented
path. Reassembled output decodes to exactly the original stream's YUV pixels.
All 219 default host tests and strict all-target Clippy pass. The one ignored
test is an explicitly invoked timing harness, not a skipped functional test.
Formatting and the cyclomatic-complexity limit of nine also pass.

The isolated replay uses Rust 1.90 release binaries on a Ryzen 9 7945HX with
32 allowed logical CPUs, Linux 7.0.0-31-generic and the project's Debian 12
`uscreen-ci:perf` image. The desktop stayed active; CPU affinity/frequency were
not pinned. No compilation or agent workload ran during the final series.
Software decode validation used stock FFmpeg 5.1.9 outside the timed region.

The new read replay uses the original T384 synthetic fixtures: fragmented
40-picture/512-byte-slice streams in seven-byte reads, dense 400-picture streams
in 16 KiB reads, and two-picture/1 MiB-slice streams in 4096-byte reads. Each
session repeats those streams 30, 40 and 20 times respectively, with one
unmeasured warmup. Payloads are deterministic syntax markers, not decodable
video; separate real-codec tests supply that coverage. Parser/runtime setup,
read-buffer allocation and output retirement are inside timing; input fixture
construction and worker creation are outside. Threads start behind a barrier.

Ten alternating rounds run all three implementations. Each produces eighteen
read-profile groups plus eighteen original T384 samples: 540 read groups and
540 original samples in the final artifact. Group CPU sums per-thread CPU;
wall time records each group's slowest worker. Raw ranges and per-worker
counters remain available. Do not interpret worst-worker duration as a pooled
frame percentile or the independent workers as physical connected tablets.

Initial candidates and the refinement precheck are retained separately. The
first attempted cross-revision comparison is explicitly discarded: Cargo reused
one executable across mounted source trees with older timestamps. Those numbers
are not a comparison. Final builds clean both project packages between revisions
and verify three distinct executable hashes. Source and executable hashes,
compressed counterpart adapters, the controller, every row and validation logs
are in the [artifact directory](2026-09-17-cli-assembly/).

To run the current isolated replay and regressions:

```sh
cargo test --locked --release -p uscreen --bin uscreen \
  t407_read_boundary_profile -- --ignored --nocapture
cargo test --locked --release -p uscreen --bin uscreen \
  t384_packetizer_profile -- --nocapture
cargo test --locked -p uscreen --bin uscreen
```

For cross-revision reproduction, export `1e7b045` and `8305f04`, use the matching
compressed Annex B/profile/read-adapter snapshots, and build each in an isolated
output directory or clean `uscreen` and `uscreen-config` first. The archived
build/controller scripts record the exact calls and alternation. Preserve the
reported toolchain, verify hashes and compare variants within the same series.
