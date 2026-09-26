# Explicit Windows runtime-file ownership (T637)

The permanent native token and lease tests failed in
[run 36264487601](https://github.com/geraldo-netto/UScreen/actions/runs/36264487601)
at `374bdba`: `user.matches(owner)` was false for newly published runtime files.
Windows selects the process token's default owner unless creation supplies an
explicit owner; that SID can differ from the current user. This contradicted
Blent's owner-only runtime contract. See Microsoft's
[new-object ownership contract](https://learn.microsoft.com/en-us/windows/win32/secauthz/owner-of-a-new-object).

Token, lease record and lock creation now reopen the same owned file object with
`WRITE_OWNER` and assign the current user before writing/publishing state. The
private directory's inherited owner-only DACL remains intact. No path is reopened
to select a potentially different file. Token creation borrows the caller's
validated directory, so tests and future lifecycle code retain explicit ownership
instead of reopening global token state. Location lookup has no write side effect.

The same owner assertions, plus single-user access-control checks, pass in
[run 36264925943](https://github.com/geraldo-netto/UScreen/actions/runs/36264925943)
at `44b939e`. `set_file_owner` and token creation measure 100% native executable-line
coverage. Atomic token replacement, failed publication cleanup and lease-record
retirement checks remain in the normal suite. The unrelated junction fixture now
uses Windows command-line quoting correctly; it also passes.

Evidence: [red assertions](artifacts/2026-09-26-windows-development/t637-red.log),
[native results](artifacts/2026-09-26-windows-development/t493-native.log),
[per-function counters](artifacts/2026-09-26-windows-development/t493-functions.json),
[raw counters](artifacts/2026-09-26-windows-development/t493-windows.lcov).
Windows application capabilities remain disabled; Windows 11 VM repetition is T633.
