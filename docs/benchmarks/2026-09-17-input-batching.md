# Native input batching — 2026-09-17

T406 preserves every native input event and each existing `SYN_REPORT` boundary,
while collecting the events of one synchronization frame into a bounded write.
It does not wait for another pen/touch sample. Proximity, button and tip changes
retain their separate frames; controller ownership and release behavior remain.

The recorded replay contains 256 events in 38 frames. With a writer accepting
each complete request, writes fall from **256 to 38 (85.2%)**. Short writes can
require more calls and are tested separately. This is a counting-writer result,
not a measured reduction in physical input latency or total host CPU.

| Device | Native events / complete writes before | Synchronization frames / complete writes after |
| --- | ---: | ---: |
| pen | 100 | 23 |
| touch | 150 | 13 |
| parked pointer | 6 | 2 |
| total | 256 | 38 |

The pen replay covers normal/eraser tools, hover, proximity, side-button changes,
tip down/move/up, exit and teardown with a held button. Touch covers all ten
slots, movement, primary-contact replacement and release. Repeating teardown
must emit nothing extra. A pen-down with a button transition remains three
frames: six proximity events, two button events and three tip events; eleven
individual writes become three requests of 144, 48 and 72 bytes on this host.

## Counting replay

[`input-batching.py`](../../scripts/benchmarks/input-batching.py) builds the
recorded immediate emitter and current batch writer in separate target
directories. Each of 1/2/4 independent worker sessions replays the same event
sequence 10,000 times. The sink counts writes, total bytes and an ordered rolling
byte hash, without opening `/dev/uinput` or injecting desktop input.

| Sessions | Complete writes, before → after | Identical output bytes |
| ---: | ---: | ---: |
| 1 | 2,560,000 → 380,000 | 61,440,000 |
| 2 | 5,120,000 → 760,000 | 122,880,000 |
| 4 | 10,240,000 → 1,520,000 | 245,760,000 |

Per-device byte counts and hashes match in all lanes. The normal automated
fixture additionally compares the entire output byte-for-byte, including zero
timestamps/padding and every synchronization event. Counts scale with replay
sessions; this does not establish real multi-tablet input throughput.

The baseline emitter was extracted without changing behavior from `f0f92e1`.
Before buffering, 35 input tests passed, including the recorded byte fixture;
three batching assertions failed as expected. Their same assertions pass after
buffering. The baseline emitter source and extraction/red/green logs are
retained with [raw replay results](2026-09-17-input-batching/).

```sh
python3 scripts/benchmarks/input-batching.py --output /tmp/input-batching.json
cargo test --locked -p uscreen --bin uscreen input::
```

The replay needs Linux, a C/Rust toolchain and available cached Rust dependencies.
Its recorded run used Rust 1.90 in the Debian 12 `uscreen-ci:perf` container.
No elapsed-time or CPU comparison is claimed from the counting sink.

## Ownership, bounds and failure behavior

`input::event_writer` owns a reusable, zero-initialized buffer of 64 native
records per device (1,536 bytes on the measured x86-64 ABI). The largest current
synchronization frame is 24 records when releasing ten touch slots. Serialization
uses libc's native structure size/field offsets; it does not read uninitialized
Rust padding. A compiled Linux-header fixture checks the actual ABI on the test
host. This run does not validate a 32-bit platform.

Each `syn()` appends its existing synchronization event and calls `write_all`,
then preserves the original flush call. Interrupted and short writes retain
byte ordering. Pending storage is retired before I/O: after an error, later
samples cannot replay already-written prefixes. Already accepted kernel events
cannot be rolled back; a batch write is not a transaction across kernel errors.
An oversized batch fails before output, and dropping a device does not publish
an unfinished frame. Existing controller teardown still sends explicit releases
while the device is owned.

Ten permanent T406 tests cover the recorded pen/touch/pointer sequence, native
ABI/padding, intentional frame boundaries, immediate flush at synchronization,
short/interrupted writes, prefix errors, zero writes, flush failures, bounds,
reuse and unfinished teardown. Existing T318 protocol/button/proximity and
controller replacement regressions remain unchanged apart from constructing
the extracted writer interface. Input tests pass (44); full default host tests
pass (244 plus one manual benchmark ignored). The optional encoder suite passes (223 plus two manual benchmarks ignored),
and both feature configurations pass strict Clippy. The complexity gate reports
2,437 functions with none above nine. Logs are retained with the artifacts.
