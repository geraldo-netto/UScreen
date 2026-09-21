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

## T533 — distinguish dependency evidence and backend readiness

Windows `doctor` and the GUI use the same shared diagnostic report. ADB and
FFmpeg report executable paths and parsed versions; missing executables,
failed or malformed version checks, unsupported backends and unverified runtime
capabilities remain distinct. A found dependency never marks a tablet connected
or an encoder usable. `doctor` retains its unsupported-backend failure exit.
The GUI reuses its ten-second capability cache and background worker; each
version command uses the platform process adapter with a two-second deadline.

Permanent tests exercise missing commands without execution, Unicode paths,
both version formats, command failures, non-UTF-8/oversized/malformed output,
all 256 single-byte token mutations, subprocess failure/timeout and identical
CLI/GUI evidence. Existing Windows CLI expectations remain and now also require
FFmpeg/version/unverified-connection output. A Windows status regression forbids
claiming a running daemon or connected tablet from version discovery. The
initial portable contract test could not import the not-yet-created diagnostic
module; this was an API check for new functionality, not a behavioral-bug red.

The common/GUI normal suites pass 179 tests with none ignored. All 73 functions
in the changed portable/Linux measurement scope meet 80% line coverage.
Formatting, Linux Clippy and GNU/MSVC all-target host/GUI Clippy pass, retaining
only the existing T573 parity-lint exception on the command line. Complexity
remains at most nine. The shared policy also checks on WebAssembly without
default features. [Evidence](2026-09-21-follow-up-evidence/t533.tar.gz) contains
the initial contract failure, final test/check logs, source manifest and scoped
coverage. Windows-native execution and per-function counters remain unavailable
under T493/T497; cross-target compilation is not native acceptance. Display,
input, camera and lifecycle capabilities remain disabled on Windows.

## Final Linux and Android reload

After T523 and T533, source commit `662eb35` was packaged with stock FFmpeg
6.1.6 and stock libevdi 1.15.0. The C helper and Android APK sources were
unchanged from the earlier accepted update. A clean Debian 12 AppImage smoke
run passed dependency resolution, bundled encoder startup, user registration,
GUI launch and independent daemon/GUI lifetime checks before installation.

The latest AppImage and existing signed Android APK were reinstalled and both
applications reloaded. Installed AppImage hash and pulled Android APK bytes
match their build artifacts. Configuration stayed byte-identical: VAAPI
constrained-baseline, 1280×800, 30 FPS, adaptive idle off. The same Xorg process
(PID 2512, started September 20) survived; no module reload, desktop-session
switch, Android lock, power action or ADB-server reset was used.

Ten consecutive five-second host reporting windows contained 507 accepted
render ACKs and zero aged-out entries. Their packet-ready-to-render-ACK medians
ranged from 17.7 to 18.6 ms, with p95 from 19.9 to 23.9 ms. This confirms
post-reload progress; the live desktop workload was uncontrolled, so these
numbers are not a before/after speedup, capture-to-display latency or adaptive
idle acceptance. Stock EVDI's known initial startup delay still occurred.

| Installed artifact | SHA-256 |
|---|---|
| AppImage | `5af9ba1264821061cc9a76751dbffc9c1d632a8a0f7a7aa565654485feacef8c` |
| Corresponding source archive | `799910c3ef4007bbbac32875bef6af1b75fa33052c77c5eabc6a962fbe2f98b5` |
| Android APK | `21ecabb00ec1553e5cde8fcab2f18416bfe82a22b06f0e684b86e60dd0a004c9` |

The preceding AppImage remains available as
`~/.local/share/uscreen/appimage/UScreen.AppImage.before-followup-662eb35`.
Source archive and APK are retained under
`~/.local/share/uscreen/updates/followup-662eb35/`.
[Activation evidence](2026-09-21-follow-up-evidence/final-activation.tar.gz)
contains the clean-container checks, installation results, hashes and bounded
ACK sample. Private configuration and credentials are excluded.

T492 remains unresolved pending the requested fixed-workload tablet window or
an explicit decision about deferring its sustained battery comparison. The
separate [T575 GPU feasibility check](../benchmarks/2026-09-21-gpu-capture.md)
does not enable a GPU capture backend or establish a latency gain.
