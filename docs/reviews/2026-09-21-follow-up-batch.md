# Requested follow-up implementation

T418 shared input and its matched latency measurements are recorded in the
[dedicated report](../benchmarks/2026-09-21-shared-capture/README.md). Both
applications were subsequently reinstalled/reloaded and live FFmpeg 6.1.6
VAAPI output verified. T222 is deferred by the maintainer and no longer blocks
ordinary authorized reloads.

## T566 — report configuration changes after persistence

`storage::write_at` retains the semantic diff before writing but logs it only
after temporary-file write, synchronization and atomic replacement succeed.
Unchanged saves remain quiet. The new permanent Linux failure regression reads
an existing configuration through procfs, where creating a replacement is
forbidden even as root; it failed with the premature success message before
the fix, then passed after it. A portable success/no-op regression verifies
the saved value and qualified diagnostic behavior.

Validation: 91 common tests and both existing host diagnostic regressions
passed; the changed method measured 18/20 executable lines (90%). Complexity
remains at most 9. [Evidence](2026-09-21-follow-up-evidence/t566.tar.gz) retains
red/green logs, source manifest and the scoped coverage report. Other methods
in that common-only report are outside this change's measurement scope; it is
not a replacement for the full-platform coverage gate.

## T567 — normalize forwarding-module formatting

`rustfmt` normalized the existing forwarding implementation and tests without
changing behavior. The normal `scripts/format-rust.py --check` failed on this
module before formatting and passes for every source root afterward. No
artificial behavioral tests were added for the formatting-only change.
The [before/after logs](2026-09-21-follow-up-evidence/t567.tar.gz) preserve the
original drift and successful check.

## T568 — retain progress for the acknowledged output

Each tracked packet now records its encoder output ordinal. A render ACK advances
that encoder's progress only through this ordinal, so an ACK for an older frame
cannot erase newer pending outputs or disable the subsequent stall watchdog.
Render samples retain the same ordinal, including unpublished-output gaps.

Four permanent regressions failed before the change and pass afterward: delayed
ACKs across sequence wrap, unpublished outputs, a bounded 64-case invalid/duplicate
ACK corpus, and a paused-clock watchdog test after encoding stops. The default
host suite passes 488 tests; the optional encoder's latency suite passes 11.
Three existing default-suite ignored tests are opt-in benchmarks, not skipped
regressions. All 30 measured production methods in `latency.rs` meet 80%
executable-line coverage. Formatting passes; 5,343 functions remain at or below
cyclomatic complexity 9.

Ordinary strict Clippy also reports the pre-existing test parity expression
tracked in T573. With only `manual_is_multiple_of` temporarily permitted on the
lint command line, all-target host Clippy passes; no source lint suppression was
added. This is a scoped lint result, not an unqualified clean strict-lint claim.
[Evidence](2026-09-21-follow-up-evidence/t568.tar.gz) includes red/green logs,
the final source manifest, scoped coverage and lint/format/complexity results.

## T523 — share host sessions and protocol ownership

The host library exposes attachment credentials and retirement, control/video
servers, bounded encoded storage, latency accounting, selection data and session
orchestration on all host targets. Linux uses this library through a small
capture adapter and its existing input backend. Native EVDI card identity now
belongs to the Linux input adapter. The shared session binds both listeners
before starting capture and owns every returned worker through shutdown.

Token generation uses stock `getrandom`; the existing comparison algorithm
remains shared. Native private-token persistence stays in platform services.
Stock `socket2` replaces the transport's Unix-only send-buffer syscall with the
same best-effort 128 KiB request. No wire format, encoder policy, display default
or Android behavior changed.

Five new portable integration tests cover credential rotation and retired
leases, delayed ACK ownership, authenticated TCP/WebSocket bytes and render
receipts, partial startup cleanup and cancellation of injected capture/input
workers. The first interface test did not compile before extraction because
the library did not expose these modules; that is an API-boundary check, not a
claim of reproducing a behavioral defect. Existing Linux regressions remain in
the normal suite. A bounded credential mismatch corpus and entropy-failure
test cover the new portable entropy adapter.

The default workspace run passes 778 tests, with three existing opt-in
benchmarks ignored. GNU and MSVC all-target host checks pass. These compile
checks do not validate native Windows capture, input, ACLs or lifecycle; Windows
capabilities remain disabled and T493/T497 retain native validation requirements.

The optional in-process host run passes 496 tests, with two existing opt-in
benchmarks ignored. Combined default/optional coverage meets 80% for every
changed or moved production function. The full Linux report still exposes the
two pre-existing camera gaps tracked in T572; 58 Windows-only methods remain
unmeasured in the all-platform report. Formatting and complexity pass (5,366
functions, none above nine). Default and optional all-target host Clippy pass
with only the existing T573 `manual_is_multiple_of` finding permitted on the
command line. [Evidence](2026-09-21-follow-up-evidence/t523.tar.gz) retains both
test logs, platform checks, source manifest and coverage reports. Native Linux
display behavior was tested through existing isolated regressions; this
refactor was not reloaded into the live display during these checks.
