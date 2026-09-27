# Windows interactive daemon lifecycle — T524

The daemon and GUI now share the Windows lifecycle adapter in
`common/src/windows/lifecycle.rs`. Shared bounded-wait and restart policy lives
in `common/src/lifecycle.rs`; Linux restart uses the same stop-before-start rule.
Native paths, ACL validation, process identity, file publication and child jobs
remain behind their platform boundaries.

`blent start` and the default invocation run in the current interactive session.
`blent stop` requests graceful retirement for the exact PID/creation-time/executable
identity. `blent status` accepts only matching ready and live lease records.
The GUI launches independently, waits at most five seconds for readiness, and
retires unsuccessful startup processes. Restart never starts after a failed stop.
Daemon workers retain the validated kill-on-close job ownership used by native
streaming commands. Stopping the GUI does not stop a successfully launched daemon.

Runtime state lives in the current user's native LocalAppData `blent` directory.
A hidden `--runtime-dir` override supports isolated instances and tests, with the
same absolute-path, ACL and reparse-point checks. Read-only status opens existing
state without creating it. Session tokens rotate after exclusive lease acquisition;
ready and stop records publish atomically. Graceful stop and cancellation retire
owned token/session records. Forced process termination releases kernel locks and
jobs; the next start or stop reclaims abandoned files under a new exclusive lease.
Replacement identity records are preserved. An independent cleanup failure must
not leave another removable secret behind or make stop report successful cleanup.

Display, input, camera, ADB connection, tray and system setup remain
unsupported on Windows. Optional [per-user autostart](2026-09-27-windows-autostart.md)
now uses this lifecycle and preserves existing daemon identity at repeated login. GUI labels say daemon rather than display service on this
backend. Diagnostic command success means the report ran, not that streaming works.

## Permanent tests and evidence

- `host/tests/windows_lifecycle.rs`: concurrent start, default/explicit start,
  Unicode paths, exact status, restart, startup errors/deadlines, crash recovery,
  stale stop requests, no-side-effect absent status/stop, and cleanup-error reporting.
- `common/src/windows/lifecycle_tests.rs`: cancellation with an owned native child,
  malformed/oversized JSON corpus through 100,000 bytes, stale ownership, partial
  startup, independent resource retirement and preservation of replacement state.
- `gui/src/platform/windows.rs` and `gui/src/windows_tests.rs`: isolated GUI command
  routing and error propagation, truthful labels/capabilities, retained settings tests.
- `common/src/lifecycle.rs`: bounded transitions, probe errors and restart ordering.

Native red run [36267606020](https://github.com/geraldo-netto/UScreen/actions/runs/36267606020)
proved the old application lacked lifecycle startup/status. Native full-workspace
run [36269676965](https://github.com/geraldo-netto/UScreen/actions/runs/36269676965)
passed the initial implementation and measured all 77 Windows-specific production
functions at the 80% threshold. Review then found an independent cleanup failure
could retain the token and make stop falsely succeed. Permanent native regressions
in [36271039059](https://github.com/geraldo-netto/UScreen/actions/runs/36271039059)
failed with both exact failures before the cleanup fix. Final native run
[36271915764](https://github.com/geraldo-netto/UScreen/actions/runs/36271915764)
at `f5cb33cea4e198b025bb0a3a1aa5fbb0dcf42cee` passes workspace tests, all-feature
builds, GNU linking and native coverage. All **80 Windows-specific production
functions** meet 80%; native source fingerprints match the committed application
sources. [Retained functions, hashes and red/green logs](artifacts/2026-09-26-windows-lifecycle/)
make the acceptance reviewable. Shared lifecycle and unchanged Linux behavior
also pass both normal workspace configurations.

Native CI uses Windows Server 2022. The retained Windows 11 development VM runs
its own workspace acceptance under T633; physical USB/GPU/display acceptance
remains separate under T522. These isolated tests do not claim working streaming.
