# T607 — camera write-chunk sweep

Keep the production 8 KiB chunk. Smaller writes cost more CPU locally. Larger
writes reduce synthetic sink-call overhead, but did not produce a useful ADB
throughput improvement on the attached tablet. No production packet, flush,
protocol, timeout or memory limit changed. T608 can therefore study transport
and frame age against the existing bounded writer.

## Results

Native ART, Okio **3.6.0**, one warmed process, eight rotating/reversed repeats
per candidate. Each repeat writes 1,200 packets: forty 1 MiB and 1,160 64 KiB
payloads, plus their four-byte lengths. Flush once per packet. A separate
63-case differential check compares exact bytes and source-buffer position/limit
against production. The runner refuses algorithm drift between the benchmark
writer and `CameraWire.packet`.

| Chunk | Writer CPU, median ms | ART allocated KiB | Logical staging peak, bytes | Sink calls |
| --- | ---: | ---: | ---: | ---: |
| 512 B | 154.69 | 80 | 8,196 | 15,600 |
| 1 KiB | 100.17 | 64 | 8,196 | 15,600 |
| 2 KiB | 73.42 | 64 | 8,196 | 15,600 |
| 4 KiB | 60.33 | 64 | 8,196 | 15,600 |
| 8 KiB | 54.38 | 64 | 8,196 | 15,600 |
| 12 KiB | 51.77 | 64 | 16,388 | 11,600 |
| 16 KiB | 50.25 | 64 | 16,388 | 8,400 |
| 32 KiB | 47.43 | 64 | 32,772 | 4,800 |
| 64 KiB | 53.85 | 12,048 | 65,540 | 3,000 |

All candidates retained zero logical buffered bytes after packet flush. No
workload GC was reported; forced boundary collections are excluded from that
count and retained in raw data. ART counters are process-wide and published at
GC boundaries; these are descriptive medians, not isolated allocator accounting.
Logical staging excludes segment capacity rounding, segment pools and native
memory. Sink calls are **not** network syscalls or USB packets.

![Synthetic chunk tradeoffs](artifacts/2026-09-26-camera/graphs/chunk-local.png)

The 32 KiB writer used 12.8% less synthetic CPU than 8 KiB, so an actual ADB
follow-up was justified. A generated 1280×720/30 FPS baseline H.264 clip at
3 Mbit/s supplies 300 complete access units, replayed ten times per connection.
This matches the default **camera** profile; the tablet's 1280×800 **display**
profile is separate. Socket settings match production: TCP_NODELAY, requested
128 KiB send buffer, per-packet flush. The receiver checks every byte before its
final packet-count ACK. Eight interleaved repeats per candidate, sensor off. The final sweep adds the
maintainer-requested 12 KiB point: 4.8% less synthetic CPU than 8 KiB, but
16,388-byte peak staging and no ADB throughput benefit.

| Chunk | USB/ADB throughput, median Mbit/s | Android writer CPU ms / 3,000 packets |
| --- | ---: | ---: |
| 8 KiB | 220.93 | 523.49 |
| 12 KiB | 218.69 | 528.66 |
| 16 KiB | 220.89 | 538.58 |
| 32 KiB | 216.43 | 526.96 |
| 64 KiB | 220.55 | 518.98 |

All **120,000/120,000** final untraced packets matched and were acknowledged.
Neither 12 nor 16 KiB improved throughput; 32 KiB was about 2% slower in this
repeat, reversing its small earlier lead. Android CPU medians overlap the
individual-run spread. This maximum-throughput replay is not a live
camera latency or battery benchmark. It includes a deliberately validating
Python receiver and the normal host/tablet environment. Avoid extrapolating
synthetic 12.8% savings to the complete camera pipeline.

![ADB chunk comparison](artifacts/2026-09-26-camera/graphs/chunk-adb.png)

The initial eight-size synthetic and four-size ADB campaigns are also retained
in `t607/initial/` (96,000 additional byte-exact untraced packets). They support
the same default decision. A separate traced 32-run campaign also verified 96,000 packets. Receiver-only
strace counted 148,864 `recvfrom`, 64 `sendto`, 729 `read` and two `write` calls;
0.503 seconds aggregate reported syscall time (not an application CPU profile). These aggregate receiver counts do
not identify Android write syscalls, ADB daemon calls or per-candidate counts.
Traced throughput is retained separately, never mixed with untraced timing.

The 512-byte USB bulk maximum, network MTU and Okio segment size constrain
different layers. Smaller application chunks still produced the same 8 KiB
sink emissions. Alignment alone predicted neither allocation nor USB benefit.

## Reproduce and retained evidence

```sh
ANDROID_HOME=/path/to/android-sdk ./android/gradlew -p android --offline :app:assembleProfile
adb -s SERIAL install -r android/app/build/outputs/apk/profile/app-profile.apk
python3 scripts/benchmarks/camera-chunks.py --serial SERIAL --output /tmp/chunks-new
python3 scripts/benchmarks/camera-fixture.py --output /tmp/fixture-new
python3 scripts/benchmarks/camera-socket-run.py --serial SERIAL --fixture /tmp/fixture-new/camera-replay.bin --output /tmp/usb-new
# Repeat into another directory with --trace for receiver syscall attribution.
```

The replay runner creates only an allocated ADB reverse mapping, checks ownership
before removal, stops the profile app and restores the normal app in `finally`.
The profile APK's permission-protected development activities are absent from
normal debug/release variants. Camera service inspection afterward showed no
active clients and both devices closed. Native lifetime/bounds/cancellation tests
in `CameraPacketBufferTest` remain unchanged and pass.

[Raw results, source/APK identities and fixture recipe](artifacts/2026-09-26-camera/t607/).
[SVG/PNG graphs](artifacts/2026-09-26-camera/graphs/) regenerate with
`scripts/benchmarks/camera-chunk-graphs.py`. Exact fixture bytes are retained at
`~/.local/share/blent/profiles/2026-09-26-camera/t607/`; the fixture generator
reproduced their SHA-256 byte-for-byte with the recorded FFmpeg build.

Historical T594 whole-payload-array results used a different measurement setup;
its absolute timings are not a baseline for this warmed chunk sweep. T607 is
resolved by measurement and a retained benchmark, not a speculative default change.
