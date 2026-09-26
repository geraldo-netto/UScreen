# Portable raw-frame writes (T635, prerequisite for T529)

`host/src/raw_transfer.rs` supplies the framing/backpressure policy for native
encoder-input adapters. It accepts canonical packed NV12 geometry from the
existing shared layout contract and writes exactly one frame at a time through
an `AsyncWrite` handle. It does not create a queue or copy the input frame.

Partial writes continue until the frame is complete. An I/O failure, zero write,
deadline or cancellation drops the owned writer and retires that session;
subsequent frames cannot append to its incomplete input. Native adapters must
pass an exclusively owned input handle and retire their encoder process when
the input fails. A new session receives a new writer. Geometry changes require
a new encoder configuration; incorrect dimensions or lengths are rejected
before writing any bytes.

The default write/shutdown budget is five seconds. Callers can select a nonzero
budget up to sixty seconds: shorter budgets recover sooner from stalled
encoders; longer budgets tolerate larger pauses. The future Windows adapter
must expose this choice through the shared capacity configuration. These are
pipe-write deadlines, not measured presentation latency or a UI setting already
available in the current application.

Seven permanent tests cover partial writes, paused-time stalls, cancellation,
consumer exit, malformed lengths, invalid geometry, resize rejection, zero
writes, shutdown errors/deadlines and recovery on a new stream. Linux LLVM
counters measure **100% executable-line coverage for each of the five production
functions**. Source fingerprints are retained with the report. GNU Windows
target checking and the full complexity gate also pass.

- [Test output](artifacts/2026-09-26-windows-development/t635-tests.log)
- [Per-function report](artifacts/2026-09-26-windows-development/t635-functions.json)
- [Native Linux counters](artifacts/2026-09-26-windows-development/t635-native.lcov)
- [Complexity gate](artifacts/2026-09-26-windows-development/t635-complexity.log)

This is a shared implementation prerequisite, not Windows runtime support.
T529 still needs owned native encoder processes, FFmpeg integration and Windows
pipe acceptance; complete streaming also depends on T525/T528.
