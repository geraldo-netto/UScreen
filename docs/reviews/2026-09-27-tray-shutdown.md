# T537: Linux tray shutdown ownership

`tray::run` now explicitly owns the independently spawned ksni service. Normal
channel closure awaits native shutdown; task cancellation requests shutdown from
the ownership guard's destructor. Dropping ksni's bare handle does neither.

Two permanent normal-suite regressions were added before the fix. On the original
code, normal owner exit left the StatusNotifierItem's native D-Bus name registered.
A second regression then reproduced the historical panic exactly: after owner
exit and private bus shutdown, ksni 0.3.6 reported “all futures in select! were
completed, but no `complete =>` handler was provided”. Its request receiver and
name-owner stream had both ended without an explicit shutdown request. The same
regressions pass with the ownership fix; producer closure and task cancellation
also retire the native registration. Tests use their own `dbus-run-session`;
only that process is terminated, with no desktop or EVDI attachment.

This establishes a current reproducible cause matching the historical error,
not a reconstructed trace of the September 20 incident. No dependency fork or
version change is required. The fix applies to the Linux code used by AppImage
builds; the installed running AppImage was not replaced or restarted.

Validation: all six tray tests pass, including existing menu, update, missing
watcher and stable AppImage launcher cases. Fresh Linux LLVM counters pass all
15 production functions in `host/src/tray.rs`, individually at least 80%; no
missing counters. The final test-only expansion reused that collection target
with unchanged production sources. Repository complexity check: 6,399 functions,
none above nine. Formatting and whitespace checks pass.

[Evidence](artifacts/2026-09-27-tray-shutdown/) retains both failing regressions,
passing test/coverage logs, source snapshot, raw LCOV and per-function report.
Collection: `CARGO_LLVM_COV_TARGET_DIR=<fresh-target> cargo llvm-cov --locked -p
blent --bin blent --lcov --output-path <lcov> -- tray::tests::`; the coverage
reporter checks `--scope host/src/tray.rs --rust-platform linux`. This is scoped
native evidence, not a newly collected whole-project gate.
