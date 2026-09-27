# Native Windows scheduling acceptance (T583)

The existing isolated High-priority regression exercises both owned synchronous
and asynchronous children. New permanent tests verify that changing back to
Normal reaches both child paths and that a handle lacking priority-set rights
produces an error without changing the effective class. The Windows adapter
separates current-process selection from applying a class to a handle, keeping
native permission failure directly testable without changing the caller's
process rights or relying on a privileged test account.

[Run 36263320613](https://github.com/geraldo-netto/blent/actions/runs/36263320613)
at `47efa7a` passed these regressions in the normal workspace suite and native
coverage run. `apply_current`, `apply_process` and owned-child `child_priority`
each measure **100% executable-line coverage**. All native source fingerprints
exactly match the reporting checkout; no counters from Wine or cross-compilation
are credited.

- [Native test output](artifacts/2026-09-26-windows-development/native-validation.log)
- [Per-function report](artifacts/2026-09-26-windows-development/t583-functions.json)
- [Native Windows counters](artifacts/2026-09-26-windows-development/t583-windows.lcov)

These are isolated scheduling contracts on Windows Server 2022. The retained
Windows 11 VM will repeat the full suite under T633. Unfinished daemon/ADB
integration remains T524/T525; these checks do not enable application backends
or establish latency improvements. macOS work remains declined under T584.
