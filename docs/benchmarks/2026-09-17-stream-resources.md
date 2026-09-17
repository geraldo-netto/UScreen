# Encoded-storage and slow-viewer limits (T391)

The server now bounds admitted encoded backing storage at 32 MiB per session,
closes video viewers whose framed write or retained batch exceeds one second,
and reuses an eight-packet batch deque. Four sessions retain the existing
four-tablet cap and admit at most 128 MiB of encoded backing storage. This is
**not** a limit on process RSS, raw frames, unpublished encoder/parser staging,
allocator metadata or kernel socket buffers.

## Policy and ownership

`MediaBytes` keeps immutable bytes together with their backing capacity and a
shared budget charge. A Vec contributes its capacity, not only its length.
Clones, CSD caches and slices retain one charge until the last owner drops.
Callers receive borrowed byte slices; the untracked internal Bytes owner cannot
escape. An already charged backing cannot silently move to another session's
budget. This prepares T402/T407 to retain codec packets or buffer slices without
undercounting a large allocation behind a small visible range.

Admission rejects empty/oversized frames and over-budget storage. The frame
limit matches Android's existing 8 MiB + 1 wire-length bound, including type and
sequence; CSD has the same limit without a sequence. Cached initial CSD is
validated and charged before sending. A rejected encoded frame starts an IDR
recovery interval: dependent pictures are withheld until an IDR is admitted.
The optional encoder gets its existing keyframe request; stock CLI FFmpeg keeps
its periodic IDR schedule. Sequence allocation, CSD/generation checks and ACK
meaning remain unchanged. No FFmpeg patch is involved.

Thirty-two MiB accommodates several maximum-size access units or a much larger
number of typical packets while bounding shared slow-viewer retention. The replay
below peaked at about 640 KiB per session with 64 KiB frames; unit tests separately
exercise exhaustion and whole-backing accounting. This is a conservative policy
limit, not a measured universal optimum. Sixteen video admission slots, sixteen
control slots and three-second authentication deadlines remain unchanged.

The one-second deadline covers the entire framed write, including partial
headers/payloads, and the entire batch. A timed-out connection closes; it never
resumes a partially written packet on a new socket. This prevents indefinite
retention and also bounds clients making tiny progress between individual writes.
Links that cannot complete the batch within this deadline reconnect for fresh
CSD/IDR. The normal backlog rule still selects the latest available IDR; draining
one batch cannot keep consuming an indefinitely active producer.

## Measurements

[Raw results](2026-09-17-stream-resources/results.json) include every receiver's
samples, allocations, sampled resource counts, source hashes and initial unpinned
observations. [SHA256SUMS](2026-09-17-stream-resources/SHA256SUMS) covers the JSON
and [baseline replay patch](2026-09-17-stream-resources/baseline-replay.patch).
Baseline is `4062cf2`; candidate is this commit's implementation.

The resource replay runs one, two and four real loopback video/control servers.
Each has a fast video reader, an authenticated control connection, a stalled
video reader with a small receive window, pending video/control authentication,
and eight rejected video reconnects plus control connection churn. It publishes
120 synthetic 64 KiB payloads at 17 ms intervals, changes CSD and encoder generation
halfway, and crosses u32 sequence wrap. The receiver verifies complete bytes and
CSD/IDR ordering; it does not decode compressed video or send render ACKs. All
virtual input devices are disabled. No EVDI, ADB, encoder or desktop is involved.

Five alternating baseline/candidate pairs ran on CPU 8 of the Ryzen 9 7945HX,
using Rust 1.90 debug test binaries in `uscreen-ci:perf`. Odd trials ran baseline
first; even trials ran candidate first. No compilation ran during these paired
trials. This is one host and a short synthetic workload, not a hardware capacity
claim. Earlier unpinned samples were noisy and are retained in the raw JSON.

The time starts immediately before payload construction/publication and ends
after the fake reader receives and verifies the bytes. Both endpoints share one
Tokio reactor and monotonic clock. It includes allocation, scheduling, transport
and debug-build payload verification, and excludes capture, encoding, physical
USB/Wi-Fi, Android decoding and rendering. The p99 column is the median across
five trials of each trial's worst fast-reader p99, using nearest-rank percentiles.

| Sessions | Worst-reader p99 ms, baseline → candidate | Phase allocations, baseline → candidate | Sampled peak FDs / Tokio tasks | Candidate admitted peak per session |
|---|---:|---:|---:|---:|
| 1 | 3.639 → 3.326 | 3569 → 3510 | 22 / 9 | 655,362 bytes |
| 2 | 4.385 → 4.164 | 5556 → 5437 | 34 / 18 | 655,362 bytes |
| 4 | 5.618 → 4.668 | 9528 → 9290 | 58 / 36 | 655,361 bytes |

Allocation counts are medians and cover the traffic phase on the test thread,
including fake clients, JSON/control churn and `/proc` resource sampling. They
exclude setup and external native allocations. The accounting wrapper adds an
owner allocation per backing; these totals include that cost. Tail samples
varied substantially between trials, so the table does not establish a general
latency speedup. Every fast reader received all 120 frames and both configurations
in every trial. Both versions returned to ten process FDs and zero Tokio tasks
after each case. The candidate also retired the stalled viewer before shutdown;
the baseline retained it until explicit server shutdown. RSS stayed in the same
approximate range; no RSS reduction is claimed. Baseline retained-byte telemetry
was unavailable and is represented as null, not zero.

A separate 10,000-batch replay with eight packets per batch recorded:

| Implementation | Allocations | Reallocations | Requested bytes |
|---|---:|---:|---:|
| Original Vec batch | 10,001 | 20,000 | 10,400,024 |
| Reused deque | 1 | 0 | 24 |

That residual allocation is shared-byte ownership setup. Reuse removes repeated
batch allocation; it is not a claim of allocation-free encoding or networking.

## Reproduction and regression coverage

Run inside the project's isolated Rust test environment:

```bash
cargo test --locked -p uscreen --bin uscreen t391_ -- --nocapture
cargo test --locked -p uscreen --bin uscreen --features inproc-encoder t391_
```

For the baseline resource replay, apply the linked test-only patch to a detached
`4062cf2` checkout and run the resource test. It adapts only sender construction,
missing retained-byte telemetry and the expected retained stalled viewer; the
workload is the same. To reproduce the paired observations, build each test
binary once, save separate copies, and run the exact
`stream::resources::t391_one_two_four_sessions_release_resources` test alternately
with CPU affinity 8 and `USCREEN_T391_REPLAY_CHILD=1`. The normal suite runs it in
an isolated subprocess automatically. Avoid competing compiler/benchmark jobs.

Permanent regressions prove stalled frame/config writes fail within the deadline
and initial CSD charges its full backing before any frame. Both failed before
their fixes. Contracts cover final-slice release, duplicate CSD accounting,
cross-session rejection, exhausted budgets, IDR-only recovery, oversize rejection,
unauthenticated admission, codec/generation changes, sequence wrap and socket/task
release. Existing framing, vectored partial-write, generation retirement,
controller and FIFO recovery regressions remain in the normal suites.

Pre-publication parser/encoder assembly remains outside admission accounting;
T407 explicitly includes bounds for unfinished NALs/access units/CSD. Larger
physical multi-tablet, Android and sustained power experiments remain T382's
separate work. These results do not justify raising the four-tablet limit.
