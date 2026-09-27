# Registry handle-release assertion (T659)

Replacing `common/src/windows/registry.rs::Key::drop` with a no-op survived the
original Windows tests. The permanent normal-suite regression
`t659_registry_drop_closes_owned_handle` runs a dedicated child process, creates
a private registry key, verifies its live handle, drops its owner, then requires
`ERROR_INVALID_HANDLE` when querying that same handle. It also deletes its fixture.

The native ordinary-user baseline passes. The no-op destructor mutation fails
the new regression, while unchanged production closes its handle correctly.
This improves assertion sensitivity without changing production behavior.

[Retained evidence](artifacts/2026-09-27-mutation-resume/t659/) contains selected
before/after outcomes, baseline/test logs, source diffs, native identity and
source hashes extracted from the two complete 41-mutant Windows campaigns.
No project-wide mutation percentage is inferred.
