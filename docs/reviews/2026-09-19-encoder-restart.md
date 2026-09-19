# T498: raw-frame alignment after encoder restart

The workspace run reproduced 524,288 incorrect luma samples in the existing
T429 real-FFmpeg restart regression. An isolated repeat passed, so the failure
was timing dependent. Encoder-only changes signalled the previous encoder
without waiting for its exit and retained the same FIFO inode. A replacement
reader could consume a queued frame suffix or overlap the retiring reader.

`capture::ownership_tests::t498_encoder_restart_retires_reader_and_queued_frame_suffix`
deterministically failed before the fix: it pins both old endpoints, queues a
suffix, and requires a different FIFO plus a reaped reader before restart.
It passes after joining the old encoder and rotating the owned FIFO for
settings, mode and partial-write resets. The helper stays attached. Its next
writer opens the new inode and starts a whole frame; late reset announcements
for an older inode cannot rotate the current FIFO.

Permanent boundary tests also cover missing/replaced FIFO ownership, stale
reset identities, unsigned numeric limits and a bounded byte-mutation parser
corpus. These do not establish exhaustive fuzz or project-wide 80% coverage;
T497 retains that separate requirement.

Validation: 70 default capture tests passed (one existing ignored test), three
focused T498 tests passed, and 23 optional in-process capture tests passed.
The complexity check reported 4,028 functions and none above nine. In-process
validation used the existing extracted stock FFmpeg development files via
`PKG_CONFIG_PATH=/tmp/uscreen-build-deps/ffmpeg/usr/lib/x86_64-linux-gnu/pkgconfig`.
No real EVDI attachment, tablet operation or installed-app restart occurred.

[Red/green evidence](2026-09-19-encoder-restart/) preserves the deterministic
failure, default capture result, extra boundary checks and in-process result.
