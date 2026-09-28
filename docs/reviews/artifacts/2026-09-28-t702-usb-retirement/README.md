# T702 USB retirement evidence

The permanent `usb::tests::t702_*` regressions first failed against the original
production code: replacement B had no session; shutdown retained bound ports.
After the fix, Linux host library tests passed (208), and ordinary-user native
Windows host library plus portable/native USB tests passed (97 + 1 + 1).
Both independent scoped coverage gates pass all 21 functions in
`host/src/usb/connection.rs` and `host/src/usb/monitor.rs` at >=80% executable lines.
Fresh LLVM targets were used on each platform. Whole-project cyclomatic gate:
6,519 functions, none above 9. No physical tablet was used; T522 retains that gate.

Retirement invalidates authentication, removes the runtime and stops its listeners
before attempting route cleanup. Failed cleanup retains route ownership separately.
Inventory-driven retries cannot remove replaced/foreign mappings, and a serial
with unresolved cleanup cannot acquire a new session. Shutdown still reports
unresolved ownership after releasing runtime capacity and can be retried.

Decompress the manifest, counters and source hashes; replay each report using
`scripts/coverage/report.py check`, the corresponding `--rust-platform`, and
`--scope host/src/usb/connection.rs --scope host/src/usb/monitor.rs`.
Windows also requires `--native-sources windows-sources.json --prefix C:\src\blent`.
The manifest/source hashes identify the exact measured production revision.
