# Tray GUI after an AppImage update — T550

The updated image contained the Cameras tab, but the preserved display daemon's
tray still launched `uscreen-gui` beside its own executable in the old extraction.
The reported GUI's parent was that daemon, and its executable hash differed
from the installed GUI. Starting the stable installed launcher with `--gui`
opened the updated Camera tab immediately.

The Linux tray now validates `USCREEN_APPIMAGE_LAUNCHER` and invokes it with
`--gui`. This starts the current image and gives the GUI its own runtime lifetime.
An invalid configured image produces a logged error instead of silently opening
the stale sibling. Non-AppImage installations retain sibling/PATH discovery.

The permanent `t550_appimage_tray_uses_updated_stable_launcher` test reproduced
the stale sibling launch before the fix and passed afterward. It isolates a
copied old daemon, a stale sibling and an updated launcher with spaces in its
path; missing, directory and relative launcher cases are also covered. Existing
T497 tray tests retain ordinary-installation fallback, action and bus coverage.
All four tray tests pass. Fresh tray coverage is combined with unchanged T543
baseline counters; all 1086 Linux production functions meet the 80% gate.
Formatting/clippy pass, and none of 5039 functions exceed complexity 9.

For the already-running old daemon, only its old extracted GUI entry is replaced
with a reversible forwarding script to the stable installed launcher. The old
GUI executable is retained beside it for rollback. Its daemon executable,
libraries and display session remain intact; no Android lock/sleep action is
needed. Future daemon launches use the source fix in the updated image.
The repaired legacy entry was launched on an isolated Xvfb display and opened a
GUI whose SHA-256 matched the newly packaged executable. It closed gracefully
without starting camera capture. The installed image was replaced atomically;
the user's existing camera GUI remained running, and every ADB mapping and the
display daemon's executable inode were preserved.

T552 was a separate wording issue: the maintainer confirmed mirroring works
after Restart camera. The host hint now explicitly says Start/Restart applies
settings and Apply only saves preferences. The existing Camera-tab regression
still passes; camera behavior did not change. T551 separately tracks the stale
legacy `~/.local/bin/uscreen-gui` entry left by previous installations.

See [evidence](artifacts/2026-09-20-tray-update) for regression output,
coverage, binary hashes and live-session preservation checks.
