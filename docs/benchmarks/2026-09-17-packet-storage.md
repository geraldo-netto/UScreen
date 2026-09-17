# Encoded packet storage — 2026-09-17

T402 removes the drain-time payload copy for known large packet allocations in
the optional in-process encoder. The default FFmpeg CLI path is unaffected.
Stock FFmpeg is used without patches. Unknown backing storage and small buffer
views retain the bounded copy fallback.

## Measured result

The allocation/fill/publication replay compares the stock buffer pool plus a
copy with the production owner path, including allocation, payload filling,
budget admission and steady-state release. These are medians of ten alternating
pairs per case, measured using process CPU time in a release build. Each case
retains either zero or four older outputs. The newly admitted output briefly
adds one more live charge before the oldest is released.

| Payload | Older packets retained | Copy CPU µs/packet | Owner CPU µs/packet | Change |
| ---: | ---: | ---: | ---: | ---: |
| 1 KiB | 0 | 0.203 | 0.205 | +0.9% |
| 1 KiB | 4 | 0.196 | 0.199 | +1.8% |
| 64 KiB | 0 | 1.657 | 0.719 | -56.6% |
| 64 KiB | 4 | 1.554 | 0.815 | -47.5% |
| 512 KiB | 0 | 11.354 | 3.500 | -69.2% |
| 512 KiB | 4 | 13.439 | 5.890 | -56.2% |
| 4 MiB | 0 | 279.953 | 160.132 | -42.8% |
| 4 MiB | 4 | 368.768 | 209.286 | -43.2% |

For 64 KiB–4 MiB payloads, the measured stage used about 43–69% less CPU and
performed no drain-time payload copy. The 1 KiB case continues copying: its
medians differed by about 2–4 nanoseconds per packet, with overlapping repeat
ranges. This does not establish a small-packet gain. The initial candidate
checked every packet under the allocation registry lock; the final path avoids
that lookup for small buffer views.

These percentages are **not total encoding, UScreen, FPS or battery gains**.
For example, saving about 0.94 µs on a 64 KiB packet at 60 packets/second is
only about 56 µs of CPU per second. The benefit grows with large access units
and aggregate traffic. The 64 KiB ownership threshold preserves the stock
allocator/pool for small allocation requests; it is not a universal optimum.

## Ownership and memory accounting

`encoder_storage.rs` installs the public `get_encode_buffer` callback only for
codecs advertising `AV_CODEC_CAP_DR1`. Requests below 64 KiB delegate to the
stock allocator. Larger requests allocate with `av_malloc`, include and zero
`AV_INPUT_BUFFER_PADDING_SIZE` bytes, and wrap the allocation using
`av_buffer_create`. The registry records the original data range and capacity
against the public AVBuffer identity. This uses public APIs only; see the
[FFmpeg callback contract](https://ffmpeg.org/doxygen/5.1/structAVCodecContext.html).

An arbitrary `AVBufferRef.size` is insufficient for T391: references may expose
only a portion of their underlying buffer. Publication checks identity and
both reference/payload ranges against the known allocation. A small reference
view, unknown identity or replaced storage is copied into a separately owned
allocation instead. See [FFmpeg buffer references](https://ffmpeg.org/doxygen/5.1/group__lavu__buffer.html).

For a retained allocation, publication moves only the data buffer reference into
a private Packet owner. The original packet immediately releases side data and
`opaque_ref`; unrelated native buffers cannot follow the visible payload into
the queue. `Bytes::from_owner` owns the packet until the final byte reference
is dropped. Broadcast clones and slices share its lifetime and charge; no
borrow of the reusable receive-packet variable escapes. See
[Bytes owner storage](https://docs.rs/bytes/1.11.1/bytes/struct.Bytes.html#method.from_owner).

The complete known allocation, including padding, is charged to T391's budget.
A small payload inside a retained larger reference still charges the larger
allocation. Independently returned native packets conservatively get separate
charges even if a codec shares native backing; ordinary MediaBytes clones and
slices charge once. Allocator bookkeeping, native reference objects and codec
internal storage are not claimed to be covered by the encoded-data budget.
The charged maximum in the four-retained/4 MiB case was 20,971,840 bytes for
the owner versus 20,971,520 bytes for copies: five payloads plus 64 bytes of
padding per retained native allocation. This is not an RSS measurement.

Callback userdata has a stable shared Arc allocation, independent of Encoder
moves. Declaration/field order closes the codec before retiring that userdata
on construction errors and normal destruction. Buffer release uses a weak
registry reference, so a delayed consumer can safely outlive the encoder.
Registry access is synchronized; FFmpeg reference counting owns final release.
No mutable payload interface is exposed after publication.

## Validation

Nine permanent T402 tests run in the normal optional-encoder suite. They cover
large-packet copy elimination; delayed bytes across subsequent encoding and
encoder retirement; byte-identical decoded pixels from delayed large packets
versus detached copies; complete-allocation charging for a one-byte payload;
unknown/tiny-view copying; unrelated opaque-buffer release; independent charges;
and exactly-once release after slow broadcast-consumer cancellation.

The large-packet regression failed before implementation with 169,962 copied
bytes. The tiny-view regression failed before the fast-copy refinement, then
passed without weakening either assertion. The original retained-backing test
remains. Red/green logs are preserved. The full in-process host suite passed
**198 tests**; two additional ignored tests are explicitly invoked benchmark
entry points, not skipped functional regressions. Host all-target Clippy with
`inproc-encoder` and `-D warnings` passed, as did formatting and the project's
cyclomatic-complexity ceiling.

Real stock libx264 encoding and software H.264 decoding are exercised. NVENC
hardware, full Android presentation, physical power and additional FFmpeg
versions were not measured here. Non-DR1/unknown storage retains the copy path;
no hardware gain is claimed from codec API availability.

## Method and reproduction

Hardware/environment: AMD Ryzen 9 7945HX, 32 allowed logical CPUs,
Linux 7.0.0-31-generic, Debian 12 container, stock FFmpeg 5.1.9-0+deb12u1
(libavcodec 59.37.100, libavutil 57.28.100), Rust 1.90.0 release profile.
The desktop remained active; CPU placement and frequency were not pinned.
Runs were serial without concurrent agent benchmarks or compilation.

The main replay calls the actual public allocation callbacks on an opened
software encoder context, fills the synthetic packet, publishes/charges it and
retires old outputs. It does not perform video encoding inside the timed loop.
Both variants are warmed before alternating runs. Input sizes are 1 KiB,
64 KiB, 512 KiB and 4 MiB; iteration counts scale with payload size. Setup,
final retained-queue verification and final drain are outside timing; steady
release is inside. Native C allocation calls are **not** included in the Rust
allocation counters. Copy counters count the explicit drain copy only.

The simpler owner replay starts from prepared packets and excludes native
allocation from timing. It measures owner conversion and final release
separately and verifies every payload byte. Its results are supporting evidence;
the main replay includes the stock pool's reuse advantage and determines the
implementation choice. Both replays produce 160 trial rows. The preliminary
160-row boundary series and exact compressed source snapshots are retained
separately; compare variants within a series rather than treating changes
between series as a controlled timing result.

In the project's isolated CI image, run:

```sh
cargo test --locked --release -p uscreen --bin uscreen \
  --features inproc-encoder t402_packet_boundary_replay -- --ignored --nocapture
cargo test --locked --release -p uscreen --bin uscreen \
  --features inproc-encoder t402_packet_storage_replay -- --ignored --nocapture
cargo test --locked -p uscreen --bin uscreen --features inproc-encoder
```

The test output embeds `T402_BOUNDARY` or `T402_REPLAY` followed by JSON; the
marker may share a line with the test harness label. The
[artifact directory](2026-09-17-packet-storage/) preserves all trial rows,
source hashes, preliminary snapshots, validation logs and SHA256SUMS. The
physical comparison after the broader optimization batch remains T382 work.
