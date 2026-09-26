# T620 — bounded camera producer-failure fixture

The producer-failure test no longer relies on a one-second process alarm.
Its fake producers read stdin until the authenticated client completes its
handshake and writes a retirement marker. Foreign-client rejection and the
successful authenticated handshake remain mandatory assertions.

The shared fixture races server completion against the client future and has a
five-second total deadline. If the server ends before handshake, the unfinished
client future is cancelled; it cannot keep a completed server fixture alive.
The normal test requires clients to finish, so premature retirement is a failure.

The permanent `t620_early_producer_exit_cancels_pending_handshake` regression
uses a real loopback connection with no server accept, then immediately retires
the fixture server. It failed against the old join behavior at its 250 ms outer
bound and passes with cancellation. All 15 camera test-module tests pass,
including the original foreign-client/producer-failure test. Production code is
unchanged. [Red/green evidence](artifacts/2026-09-26-task-batch/t620/).
