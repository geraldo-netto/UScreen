# Replay regression monitor identity (T641)

[Run 36274495310](https://github.com/geraldo-netto/blent/actions/runs/36274495310)
failed the retained T571 ACK-order assertion after the test observed a callback
in `BLOCKED` state. That state alone does not establish that it has reached the
`ReplayStats` monitor: a class-loader or another monitor may block it earlier.
The replay implementation still publishes the ACK obligation before rendering.

The test now waits for the specific monitor identity and owning thread through
host-JVM thread diagnostics. Reflection keeps this test-only diagnostic outside
Android's compile API. The original ACK-count, completion-count and thread
retirement assertions remain mandatory.

A permanent T641 regression holds an unrelated monitor before the intended
barrier. It fails against the old state-only wait on both API 27 and 34, then
passes after checking identity and ownership. The complete replay suite and full Android debug unit suite pass.
No production or benchmark behavior changed; existing coverage source bytes
remain unchanged.
