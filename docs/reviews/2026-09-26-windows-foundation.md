# Native Windows foundation acceptance (T493)

[Run 36264925943](https://github.com/geraldo-netto/blent/actions/runs/36264925943)
at `44b939ee919df859e0afe1eb7441d416d12b99f9` passes the full native workspace
suite, the all-features MSVC build and GNU executable linking. All **46 production
functions** in `common/src/windows/*.rs` and `common/src/commands/windows.rs`
meet the required **80% executable-line coverage**; the lowest result is 80%.
All 56 common source fingerprints match the measured checkout exactly.

Permanent native acceptance covers known-folder lookup, Unicode paths,
executable discovery, owner-only protected directories, pinned directory
lifetime, actual NTFS junction rejection, token randomness/replacement and file
privacy, lease exclusion and corrupt/oversize records, process identity,
descendant retirement, missing threads and changed thread owners. Bounded ACE
truncation/mutation and malformed owner-record tests remain in the normal suite.
Tests use temporary directories and never replace a developer's live token.

T634's red/green tests fixed directory share-mode pinning. T637's native red/green
tests fixed runtime-file ownership. Additional resume tests exercise a known
retired child while retaining its handle, plus a current thread with the wrong
owner; no unrelated thread is resumed. Token creation now borrows an explicit
validated directory. The caller must retain its lifecycle lease when publishing
session state. Linux runtime behavior remains unchanged and its common suite passes.

- [Per-function report](artifacts/2026-09-26-windows-development/t493-functions.json)
- [Raw native counters](artifacts/2026-09-26-windows-development/t493-windows.lcov)
- [Source fingerprints](artifacts/2026-09-26-windows-development/t493-sources.json)
- [Native results](artifacts/2026-09-26-windows-development/t493-native.log)
- [File-ownership regression](2026-09-26-windows-file-ownership.md)

This completes the shared-service foundation on native Windows Server 2022 x64.
The retained Windows 11 VM repeats the full suite under T633. T524 lifecycle can
now proceed. T497 covers broader host/GUI counters and the combined project gate;
this scoped report does not close it. Windows capture, input and lifecycle
capabilities remain explicitly unavailable until their own adapters are complete.
