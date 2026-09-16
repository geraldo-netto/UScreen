# TODO

| id | status | severity | effort | description |
|---|---|---|---|---|
| T141 | blocked | medium | small | Resolve installer desktop-path inconsistency: `make install` quotes the GUI executable in its desktop entry, but `scripts/install.sh::install_files` emits an unquoted Exec path. Installing under a home directory containing spaces creates a broken app-menu launcher. Unblock by correctly encoding the executable path in both entry points; add a permanent sandboxed regression that launches the generated entry from a path with spaces and reserved desktop-entry characters. |
| T135 | blocked | medium | medium | Resolve GUI tablet-status contradiction: `gui/src/main.rs::poll_status` claims to select like the daemon but selects the first ready USB device, while the daemon retains actual app sessions and doctor reads their runtime ledger. A charging phone can appear as the streaming tablet. Unblock by deriving GUI active tablet status from validated daemon sessions. Add permanent regressions for an unrelated first USB device, an active Wi-Fi tablet, multiple sessions, and stale session metadata. |
