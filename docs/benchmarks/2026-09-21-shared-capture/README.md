# T418: leased shared raw input

The optional in-process libx264 encoder now defaults to shared NV12 slots.
Five matched USB replay pairs lowered conversion-start-to-render-ACK median and
p95 in every pair. This authorizes that adapter/codec default, not a switch from
VAAPI to software encoding. Stock CLI FFmpeg, including the installed VAAPI
configuration, retains FIFO; automatic NVENC input also retains FIFO until
native evidence supports another default. FFmpeg and libevdi remain unmodified.

## Before and after

Each number below is the median of five per-run statistics, in milliseconds.

| Boundary | FIFO | Shared slots | Reduction |
|---|---:|---:|---:|
| Conversion start → encoded packet, p50 | 1.2955 | 0.9387 | 27.5% |
| Conversion start → Android render callback ACK, p50 | 17.8300 | 17.3733 | 2.6% |
| Conversion start → Android render callback ACK, p95 | 21.1069 | 20.5486 | 2.6% |

| Pair | FIFO ACK p50 | Shared ACK p50 | FIFO ACK p95 | Shared ACK p95 |
|---|---:|---:|---:|---:|
| 0 | 17.482 | 17.471 | 21.085 | 20.451 |
| 1 | 17.866 | 17.494 | 21.648 | 21.098 |
| 2 | 17.830 | 17.373 | 21.107 | 20.549 |
| 3 | 17.901 | 17.081 | 21.220 | 20.555 |
| 4 | 17.608 | 16.969 | 21.028 | 20.051 |

There were 720/720 render ACKs in each run: 7,200 total, including 60 warmup
and 60 cooldown frames around each 600-frame measured interval. Android reported
no duplicate renders or invalidations. All ten encoded files had identical
SHA-256 `0c0db86cdb9aa10bd287f8ad08b96dccd7b6c458069ccb7bfdaf7e6eca07d9d1`.
Thus the transport comparison did not change encoded picture quality.
The smallest paired median improvement was only about 0.011 ms; the five-pair
median improvement is about 0.457 ms. This is evidence from this workload and
device, not a guarantee for every application or machine.

## Measurement boundary and controls

The producer runs actual `conversion.c` BGRA→NV12 conversion on repeatable
moving gradients, into either a 4-MiB FIFO or bounded shared slots. Pattern
generation precedes the timed interval. Both consumers use production Rust
input, frame ownership and libavcodec encoder code. Settings: 1280×800, 60 FPS,
30 conversion participants, libx264 CRF 18, 60,000-kbit/s configured limit,
four shared slots. Alternating pair order is FIFO/shared, shared/FIFO.
The linked distribution FFmpeg is 6.1.1, libavcodec 60.31.102, with libx264 164;
both paths use the same libraries. This is separate from the AppImage's pinned
6.1.6 CLI runtime.

USB replay uses the current Android decoder sources, MediaCodec and a physical
SurfaceView, with named `c2.unisoc.avc.decoder`, AVC Baseline, 8-bit, advertised
level 50, low-latency hint disabled, operating-rate 120, and a 60-Hz display.
The isolated replay app supplies framing and ACK collection; it does not run
the production WebSocket control session. Final packet writes follow production
vectored header/payload delivery. Host monotonic timestamps bracket conversion
start and ACK receipt. ACK timing includes USB return transport and callback
scheduling; it excludes compositor/EVDI grab and is not optical presentation
latency. It does not establish CPU, battery, YouTube/audio, full EVDI pipeline
or VAAPI gains. The existing live 30-FPS VAAPI session was left unchanged during
replay, and owned ADB routes and the production foreground activity were restored.

The decoder-project generator omits the now-required `JsonNumbers.kt` (T569).
The temporary benchmark project copied that exact production dependency before
building. This workaround does not fix the generator. Source copies and the
replay APK hash are retained for attribution.

## Earlier attempts retained

1. Host-only actual conversion/encoding: five 300-frame measured pairs showed
   FIFO packet medians 1.36–1.54 ms and shared medians 0.956–1.003 ms; all encoded
   files matched. This alone did not authorize an Android-latency claim.
2. Initial USB replay used four separate TCP writes per packet, unlike production
   vectored delivery. Shared memory was slower in all five pairs (p50 FIFO
   17.69–20.08 ms, shared 19.63–28.16 ms). These observations remain in the raw
   evidence; the transport mismatch prevents treating them as production evidence.
3. Corrected vectored replay improved p50 in four of five pairs and p95 in all
   five. The last run rendered 300 frames but returned 299 ACKs. Investigation
   identified the isolated replay completion race recorded in T571; an absent
   final ACK is not proof of an absent displayed frame. This trial was not the
   adoption gate.
4. The longer confirmation above used cooldown frames and required complete ACK
   coverage of every measured frame. All guard frames were acknowledged as well.

Raw JSON, logs, earlier attempts, benchmark source snapshots, C coverage and
red/green regression logs are in [evidence.tar.gz](evidence.tar.gz). See
[summary.json](summary.json) for exact per-run values and
[artifact-hashes.json](artifact-hashes.json) for video/replay-APK hashes.
Final test logs, source manifests and scoped/full coverage reports are retained
in [validation.tar.gz](validation.tar.gz).
Percentiles use the sorted element at `floor(0.95 * (count - 1))`; medians use
the mean of the middle two values. Join host sequence + 1 to ACK sequence and
subtract `capture_ns` from `acknowledged_ns` (nanoseconds).

## Ownership, configuration and validation

The portable descriptor bounds format, dimensions, stride, slot count, byte
length, sequence and generation. Linux handoff uses a connected private Unix
seqpacket socket and memfd with shrink/grow/seal seals. Acquire/release atomics
govern slot ownership; receiver pixels are read-only. A stock `AVBufferRef`
owns each lease, so retaining a codec reference prevents producer reuse until
the last reference is released. Resize/reconnect replace the mapping and
generation while old leases remain valid. Nonblocking release notifications
are hints; a full socket cannot permanently lose ring capacity.

`raw_transport` accepts `auto`, `fifo`, `shared_memory`; `raw_slots` accepts
2–8, default 4. The CLI build rejects explicit shared memory before capture.
Full-ring capture coalesces pending changes rather than overwriting retained
frames. Shared capture currently converts whole frames, including idle repeats;
per-slot damage/idle optimization is a separate measured follow-up (T570).

Permanent normal-suite coverage includes C/Rust malformed descriptors, bounded
mutation fuzzing, missing seals, stale sequence/generation, retained AVFrame
references, read-only buffers, full rings and wakeup, resize, cancellation,
peer crashes, invalid inherited endpoints and partial startup. Native C ran
with sanitizers; 142/142 maintained functions met 80% executable-line coverage.
The new default exposed a helper lifecycle mismatch when switching encoders:
`t418_encoder_change_reconfigures_automatic_raw_transport` failed before the
fix and passed after helper reconfiguration included effective raw transport.
FIFO-specific existing lifecycle regressions retain explicit FIFO fixtures.
An additional exec-handoff regression reproduced a fixed descriptor overwriting
an unrelated inherited descriptor. Handoff now reserves its own descriptor above
stdio before fork and clears close-on-exec only for that owned descriptor.
The Arch packaging regression also failed until its helper link command included
`raw_ring.c`; the existing local-distribution fixture now supplies that C unit.

Final checks: 767 default workspace test passes (including nested subprocess
results), 376 optional-host test passes before the final descriptor regression,
then all 11 T418 host and three portable-contract regressions passed on final
sources. The essential-script run passed 35 host-tooling tests and 268 Python
script tests; all 61 Python and 60 shell functions met the coverage gate.
All 107 functions in the scoped Rust raw transport/encoder/capture collection
met 80%. Portable configuration also compiled for WebAssembly without native
features. Complexity checked 5,336 functions with none above 9.

The broader Linux Rust report is **not a complete coverage pass**: 1,165/1,167
functions meet the threshold. Existing camera `open_device` (78.26%) and
`run_native` (28.57%) need the additional permanent adapter coverage in T572.
The report also retains 58 unmeasured Windows-only functions under T497.
Clippy leaves one pre-existing idle-policy test warning (`manual_is_multiple_of`,
T573); the new T418 code has no diagnostics. T567 retains unrelated formatter
drift. No existing ignored benchmark was enabled, removed or weakened.

The signed Android production APK rebuilt successfully with the designated
release certificate; T418 does not change Android production sources. The Linux
distribution remains the supported CLI build with bundled FFmpeg 6.1.6 and
stock libevdi. Clean Debian 12 checks passed dependency closure, encoding,
installation, idle daemon, GUI and independently extracted runtime lifetime.
Live activation remains subject to T222's existing display-reattachment risk;
package readiness is not a claim that the current live VAAPI session uses the
optional shared-memory encoder.

## Reproduction

Use a matching FFmpeg development SDK for the optional encoder, and build the
isolated decoder APK with the current decoder sources and the T569 dependency
workaround. The USB script expects that replay APK and a visible unlocked
production UScreen activity; it owns only its temporary ADB routes.

```sh
python3 scripts/benchmarks/shared-encode.py --directory /tmp/shared-confirm \
  --output /tmp/shared-host.json --build-only
python3 scripts/benchmarks/shared-usb.py --serial TABLET_SERIAL \
  --directory /tmp/shared-confirm --output /tmp/shared-usb \
  --frames 600 --repeats 5 --workers 30
```

The tests and implementation do not establish a Windows/macOS shared-memory
backend or hardware NVENC acceptance. Those require their native adapters and
measurements; portable descriptor readiness does not enable their capabilities.

## Prepared update

Final signed APK, AppImage and corresponding dependency sources are prepared in
`~/.local/share/uscreen/updates/T418-final/`. [deployment.json](deployment.json)
records SHA-256 identities and the unchanged live process identities. The final
image passed the same clean-container smoke/lifetime checks. It was assembled
before these final validation/deployment notes were added.

| Artifact | SHA-256 |
|---|---|
| `UScreen.AppImage` | `a1770c20984d48625785bbb3aaa47db1835083e83070d479d16e540be313e226` |
| `uscreen-1.2.3-AppImage-sources.tar.gz` | `8ccd3665860ab5f71d83944f8444f4eb7362f8ac708ae0d57586a43528666cbe` |
| `uscreen.apk` | `21ecabb00ec1553e5cde8fcab2f18416bfe82a22b06f0e684b86e60dd0a004c9` |
