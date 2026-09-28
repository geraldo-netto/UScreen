# T672 monitor mapping evidence

Shared tests cover physical-pixel projection, negative origins, mixed 100–300%
scaling, all rotations, normalized edges, degenerate/overflow geometry, invalid
identities, nonfinite/out-of-range input and stale topology. Snapshot identity
changes reject input until the caller releases old contacts and selects again.
Displayed Android coordinates use already-rotated desktop geometry; optional
panel-coordinate conversion explicitly applies rotation once.

Native ordinary-user Windows tests enumerate real VM monitors, verify thread DPI
context restoration and repeated snapshots, feed changed inventory through the
retirement boundary, and reject invalid handles/native mode/string fields.
Windows angles are counter-clockwise and are translated to shared clockwise
rotation names. No native display settings are changed. Synthetic topology-change
fixtures are not physical mixed-DPI/hotplug or tablet acceptance; those remain T522.

Linux passes the three focused tests and all 14 shared functions meet >=80%
executable-line coverage. Native Windows passes all 133 common-library tests and
all 22 shared/native mapping functions meet the same individual threshold.
Both collections use fresh LLVM targets. GNU cross-check and whole-project
complexity pass (6,555 functions, none above 9).

Replay decompressed evidence using `scripts/coverage/report.py check` and the
retained manifest/counters. Scope `common/src/input_mapping`; Windows additionally
scopes `common/src/windows/monitors.rs` and supplies native source fingerprints
and `--prefix C:\src\blent`. Use the corresponding `--rust-platform`.

API contracts: [physical display modes](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdisplaysettingsexw),
[monitor identity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdisplaydevicesw),
[scale factor](https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/nf-shellscalingapi-getscalefactorformonitor),
and [rotation convention](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-devmodew).
