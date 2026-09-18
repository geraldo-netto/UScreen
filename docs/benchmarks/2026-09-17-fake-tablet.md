# Fake-tablet receiver replay (T408)

The fake receiver's payload copying was a material bottleneck under controlled
fragmentation. Reusable `recv_into` storage plus bounded payload draining reduced
receiver-thread CPU by 89.4–97.6% across these cases. Traced peak allocations fell
from approximately 1.5–6 MiB to 68.2 KiB per receiver. This improves the measurement
client; it does **not** establish faster UScreen encoding, physical tablet
scalability, end-to-end latency or Android battery savings.

## Method and artifacts

Baseline: `d8141d5`'s `scripts/fake-tablet.py`. Candidate: this commit's script;
[raw results](2026-09-17-fake-tablet/results.json) record both source SHA-256 hashes,
working-tree parent, Python/platform, CPU affinity and per-client observations.
[SHA256SUMS](2026-09-17-fake-tablet/SHA256SUMS) covers the raw JSON. Measurements ran
on the Ryzen 9 7945HX Linux host, Python 3.12.3, kernel 7.0.0-31, glibc 2.39.

Reproduce from the repository root:

```bash
python3 scripts/benchmarks/fake-tablet.py --baseline-ref d8141d5 \
  --output /tmp/fake-tablet-results.json
```

The replay runs one, two or four isolated receiver processes. A barrier starts
them together; each receives one configuration and eight synthetic frame bodies
from a producer thread through an actual Unix socket pair. A wrapper limits each
socket read to the selected fragment size. ACK construction still executes, but
a counting sink replaces the control network. No daemon, EVDI output, FFmpeg,
ADB, tablet or actual compressed-picture decoding is involved.

Each case has three timing trials with tracemalloc disabled and a separate
tracemalloc trial. Receiver-thread CPU excludes producer-thread CPU. Wall time
includes socket waiting; aggregate throughput is total payload bytes divided by
the longest receiver duration, then the median across the three trials. CPU is
the median of each trial's mean per-client CPU. The allocation column is the
median per-client traced peak in the separate memory trial. Raw JSON also records
read calls, largest requested read, process peak RSS and context switches.
RSS includes interpreter and harness allocations; it is not isolated payload
retention. Source buffers are allocated before allocation tracing starts.

All baseline trials preceded candidate trials; this was not randomized or a
long sustained trial. The large differences are specific to repeated immutable
concatenation under these fragmentation sizes. They should not be extrapolated
to normal full-sized reads, TCP/ADB throughput or daemon session capacity.

## Results

Each receiver consumes 16 MiB in the large case (2 MiB frames, at most 4096 bytes
per read), or 4 MiB in the tiny-fragment case (512 KiB frames, at most 64 bytes
per read). Traced allocation peaks are reported separately from timing.

| Workload | Clients | CPU ms/client, old → new | CPU reduction | Aggregate MiB/s, old → new | Traced peak KiB/client, old → new |
|---|---:|---:|---:|---:|---:|
| Large | 1 | 525.59 → 12.50 | 97.6% | 30.4 → 1239.9 | 6146.7 → 68.2 |
| Large | 2 | 476.51 → 14.14 | 97.0% | 63.7 → 1990.9 | 6146.7 → 68.2 |
| Large | 4 | 479.82 → 12.06 | 97.5% | 123.9 → 4526.7 | 6146.7 → 68.2 |
| Tiny fragments | 1 | 1354.61 → 105.79 | 92.2% | 3.0 → 37.7 | 1538.7 → 68.2 |
| Tiny fragments | 2 | 1380.78 → 130.28 | 90.6% | 5.7 → 56.6 | 1538.7 → 68.2 |
| Tiny fragments | 4 | 1300.26 → 138.08 | 89.4% | 11.7 → 111.9 | 1538.7 → 68.2 |

## Change and contract

`read_exact` now fills one bytearray instead of repeatedly concatenating bytes.
`BufferedSocket.recv_into` consumes HTTP-upgrade leftovers before reading the
underlying socket. Video reception reuses a five-byte header and a 64 KiB drain
buffer; it parses length/type/sequence and does not retain complete video bodies.
The packet limit matches Android's current 8 MiB + 1 wire-length bound. Invalid
lengths/types and empty frame payloads stop reception without ACK; EOF during any
part of a packet cannot produce an ACK. Sequence wrap preserves the exact u32.

The normal tooling suite permanently checks binary bytes, fragmented headers and
payloads, every truncated-packet boundary, upgrade leftovers, bounded large-frame
reads, malformed packets and ACK sequences. The zero-length exception and empty
frame ACK were reproduced before fixing them; the same tests now pass. Existing
runtime-path, WebSocket and elapsed-time regressions remain.

The wire-compatible `rendered` message still uses synthetic `decode_us=1000`.
It acknowledges complete receipt only: the fake client neither decodes nor
renders. Its console output and replay metadata now say so explicitly. Use this
client for T390/T391 resource/fairness experiments, keeping synthetic observations
separate from the original USB tablet baseline. The broader T382 campaign is
closed as `wont_fix` for now; synthetic ACKs remain evidence of receipt only.
