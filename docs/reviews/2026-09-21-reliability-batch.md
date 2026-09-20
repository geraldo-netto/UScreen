# Four-item implementation batch

The maintainer requested one commit per numbered item, grouping T551/T559 in
item 1 and T558/T549 in item 3. Other findings are recorded separately in
`TODO.md`, without expanding this batch to fix them.

## Item 1 — GUI registration and configuration diagnostics

T551: AppImage registration now creates a GUI compatibility launcher that
selects the same registered image or extracted AppDir as the desktop entry.
The previous `~/.local/bin/uscreen-gui` file/link is moved into a unique
`gui-backup.*` directory inside the installation. Repeated registration is
idempotent; failed link creation restores the previous launcher. Symlink
targets remain untouched. The launcher forwards literal arguments without
embedding user paths in shell source.

T559: configuration logging compares parsed TOML values using complete paths.
An unchanged save produces no change message; changing camera bitrate reports
only `camera.bitrate`, without false display width/height/FPS changes. Quoted
keys stay distinct from nested paths. This affects diagnostics only.

Permanent tests first reproduced the stale GUI and conflicting display/camera
logs, then passed after the fixes. Retained coverage includes image/AppDir
upgrades, spaces and shell metacharacters, symlink targets, backup/idempotence,
failed-link rollback, semantic TOML equivalence, additions/removals, quoted
keys, malformed input and bounded nested paths.

Validation: 28 AppImage tests, two configuration-log regressions, two semantic
diff tests and the normal common/library suites passed. Essential-script
coverage passed for all 61 Python and 60 shell functions. Scoped Rust coverage
measured all five new diff functions at 100% and changed `write_at` at 80.95%;
this scoped run is not a replacement for the existing full-platform report.
Complexity checked 5,157 functions with none above 9.

Raw red/green logs and coverage reports are retained locally under
`/tmp/uscreen-item1-*`. New findings T566 (log emitted before persistence),
T567 (pre-existing forwarding-module formatting), and T568 (pending-frame
accounting) remain open for later review. No live daemon, display, tablet,
kernel module or installed package was restarted by this item.
