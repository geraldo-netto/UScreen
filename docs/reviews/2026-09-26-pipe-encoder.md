# Native encoder pipes (T529)

`host/src/pipe_encoder.rs` implements the libx264 software fallback using stock
FFmpeg stdin/stdout. The frame writer accepts packed NV12 with fixed geometry;
resize starts a fresh encoder session. The command recipe is shared with the
Linux FIFO adapter through `ffmpeg_args::Settings`, including color metadata,
timestamp handling, worker choices, quality settings and the existing
framecrc-plus-payload packet format. Android wire framing and codec negotiation
are unchanged. Windows hardware encoders remain T530.

`common::commands::OwnedChild` provides process ownership for both streaming
pipes and existing bounded command execution. Native Windows starts suspended,
assigns a kill-on-close job and then resumes. Linux retains its process-group
adapter. An owned encoder input carries both stdin and process ownership into
each frame-write future. Failure, timeout or cancellation retires both; no later
frame can append to a partially retired stream. The caller drains encoded output
concurrently. Shutdown observes pending writes, closes input and waits for exit
within the configured deadline. A nonzero exit remains an error.

The caller chooses the deadline (nonzero, at most 60 seconds); T635 supplies a
five-second default. There is no application frame queue. Windows Tokio 1.52.3
stdio uses a blocking worker and at most a 2 MiB staging buffer per operation;
accepted bytes therefore do not prove encoder completion. Native pipe buffers
also exist. No zero-copy, throughput or latency improvement is claimed.

Permanent tests cover partial writes, stagnant readers, cancellation, descendants,
missing executables/pipes, early encoder exit, malformed settings/frames, resize
through fresh sessions, EOF and shutdown. Stock FFmpeg encodes three synthetic
frames at each of two sizes; tests independently verify packet checksums and
decode the unchanged H.264 payload back to the expected NV12 dimensions/pixels.
No camera, tablet, display driver or active desktop is used.

Native [run 36265768764](https://github.com/geraldo-netto/blent/actions/runs/36265768764)
at `7497368` reproduced a shutdown bug: a child read only a prefix and exited
with status zero, concealing the pending Windows write error. The permanent
`t529_shutdown_reports_an_unfinished_native_write_even_after_zero_exit` regression
fails before the fix ([red evidence](artifacts/2026-09-26-windows-development/t529-shutdown-red.log)).
Shutdown now polls the native flush result before closing stdin; this observes
Windows Tokio's pending blocking write. [Run 36266085893](https://github.com/geraldo-netto/blent/actions/runs/36266085893)
at `b0cd0ae` passes the unchanged regression and the full native workspace suite,
plus the all-features MSVC build and GNU executable linking. The 17 raw-transfer, owned-child and pipe-encoder functions each pass native
Windows coverage (minimum 92.86%); all 215 common/host source fingerprints match
exactly. See [native function report](artifacts/2026-09-26-windows-development/t529-windows-functions.json),
[raw counters](artifacts/2026-09-26-windows-development/t529-windows.lcov),
[source hashes](artifacts/2026-09-26-windows-development/t529-windows-sources.json)
and [native results](artifacts/2026-09-26-windows-development/t529-native.log).

Fresh Linux collection at the same production revision passes 675 common/host
tests and all 45 scoped per-function gates (minimum 80%). Existing opt-in
benchmarks remain opt-in; no regression was skipped. See
[Linux function report](artifacts/2026-09-26-windows-development/t529-linux-functions.json),
[raw counters](artifacts/2026-09-26-windows-development/t529-linux.lcov) and
[test results](artifacts/2026-09-26-windows-development/t529-linux-tests.log).
The repository complexity scan reports 6030 functions with none above 9. The
broader T529 item remains blocked until this adapter is connected to T525 transport
and T528 capture with negotiated session ownership and user-facing deadline
configuration. Windows application display/input/lifecycle capabilities remain
unsupported. The retained Windows 11 VM repeats native acceptance under T633.

CI installs the SHA-256-pinned [Gyan stock FFmpeg 8.1.2 essentials build](https://www.gyan.dev/ffmpeg/builds/) for the mandatory native encode/decode test. The guest development seed contains only its verified CLI executable; packaging/distribution policy remains T534.
