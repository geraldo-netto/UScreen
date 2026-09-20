# T557: missing ADB display forwarding recovery

After the [T556 diagnostic collector incident](../2026-09-20-evdi-restart/README.md),
an already-ready tablet could remain assigned while its ADB reverse mappings
were absent. Android kept connecting to localhost:8890 and received connection
refused; capture and encoding still ran. Restoring the two mappings recovered
the real stream without restarting EVDI or reopening Android.

The host now checks ready display mappings on its existing ten-second recovery
cadence. Missing routes are restored through the existing per-device mutation
owner; matching routes are untouched. Conflicting destinations, malformed
snapshots and command failures fail closed. `--no-rebind` protects against a
destination appearing between the snapshot and repair. Device retirement cancels
and joins pending work before slot reuse. This does not manage camera routes.

Snapshot validation and repair selection live in portable
`common/src/adb_reverse.rs`; native command execution stays in the host adapter.
Primary and additional displays use their existing assigned host ports. Repair
does not change credentials, session ownership or Android launch history.

Permanent regression
`monitor::launch_tests::t557_ready_attachment_repairs_lost_mappings_without_reopening_android`
was added before behavior changed. It first prepares an attachment, deletes its
fake reverse mappings to model ADB server loss, then advances the recovery tick.
The original code failed with both routes absent; the fixed code restores both
custom-port mappings and keeps the Android launch count at one. Red/green logs
are retained here. Additional tests cover partial/healthy maps, conflicts,
duplicate/truncated/oversized listings, port boundaries, invalid UTF-8, failed
commands, concurrent mutations, retirement and extra-display port ownership.

Validation: all 34 monitor tests, both portable-policy tests, and the complete
default host binary suite passed (469 passed; three pre-existing opt-in
benchmarks ignored). Native LLVM coverage measured all five new production
functions at 100%; all 29 functions in the two new files plus `monitor.rs`
exceed the 80% gate. This is scoped Linux evidence, not a new whole-project or
Windows-native coverage claim. Complexity check: 5,093 functions, none above 9.

The source fix is validated with isolated fixtures; the already-running daemon
was not restarted to exercise it. Real-device recovery described above used
manual route restoration. The updated Linux AppImage is installed for the next
normal host start, with the prior T554 image retained for rollback. Release ABI
and dependency checks passed; the installed AppImage's `--version` smoke check
passed. The running service was left unchanged. [deployment.json](deployment.json)
records package/daemon/helper hashes; no Android update or additional live ADB
server reset was needed.
