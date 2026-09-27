# Private Xvfb startup diagnostics (T643)

[Run 36274495310](https://github.com/geraldo-netto/blent/actions/runs/36274495310)
reported a private-Xvfb startup timeout. The fixture discarded stderr and kept
its own copy of the display-number pipe's writer open, so an early server exit
could not reach EOF and its actual error was lost.

A permanent regression starts an owned child that emits an error and exits.
The old fixture timed out and lost that message; the corrected fixture closes
the unused writer immediately, captures bounded stderr and reports the startup
failure. It also rejects missing/non-numeric display numbers before opening any
X connection. The startup timeout and visibility/occlusion assertions are
unchanged. All 15 visibility tests pass against isolated Xvfb instances.

The original CI stderr cannot be recovered; this change fixes the demonstrated
loss of diagnostics and EOF handling, without inventing its underlying server
error. Any recurring startup failure will now retain that evidence.
