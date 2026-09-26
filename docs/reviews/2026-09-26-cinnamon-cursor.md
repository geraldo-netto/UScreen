# T604: retain the mouse while touch is enabled

Blent now arms a Cinnamon/X11 cursor policy before creating its virtual
 touchscreen. Touch stays enabled; mouse visibility is restored synchronously
when Cinnamon tries to hide it. The exact touchscreen object owns the policy;
removing it disconnects all handlers. Pending creation expires after ten
seconds. No mouse events are injected and no desktop settings are persisted.

## Cause and scope

On this Cinnamon 6.6.9 / Muffin 6.6.3 session, the cursor tracker reported
`false` while the physical mouse remained enabled. Resetting the display
layout did not repair it. Setting pointer visibility to true did; the user
confirmed the mouse became visible. Muffin's
[`on_device_added`](https://github.com/linuxmint/muffin/blob/6.6.3/src/backends/meta-backend.c#L397-L409)
hides the pointer when a slave touchscreen is added. Repeated daemon trials
recreated Blent's touchscreen and triggered that behavior.

The Linux input adapter uses Cinnamon's session D-Bus API, with a three-second
request bound, only for enabled touch on Cinnamon/X11. Other desktop backends
retain their own policies. An unavailable Cinnamon API logs a warning and
continues device creation. Restarting Cinnamon itself discards its session
hooks; a subsequent Blent attachment installs them again. This is a scoped
workaround for the observed desktop behavior, not universal desktop support.

## Permanent regression and validation

- `scripts/tests/test_cinnamon_cursor.py` runs the actual injected JavaScript
  against an executable signal model. Before the policy existed, the owned
  touchscreen addition check failed because visibility stayed false. The same
  check passes after the fix. Normal Python discovery and CI retain it.
- Lifecycle cases include repeated touch-like hiding, unrelated devices,
  matching names with the wrong type, existing-device recovery, two tablet
  policies, creation timeout, removal cleanup and bounded callback recursion.
- Rust tests cover desktop/session admission, hostile device-name escaping,
  private-bus success/rejection, missing services and bounded waiting. The
  private bus and uinput shims cannot modify the developer's Cinnamon session.
- All 70 input tests pass. LLVM coverage measures all four new Rust production
  functions at 100% executable-line coverage. The complexity checker reports
  5,738 functions, none above nine. The JavaScript is outside that checker's
  inventory; manual counting gives at most five per function.
- A Debian 12 release build and AppImage were installed. Two actual daemon
  starts created `Blent Touch`, logged successful policy installation, and
  immediately repaired an explicit native `set_pointer_visible(false)` call.
  The pointer also remained visible across device retirement. Touch, pen and
  pointer configuration stayed enabled; the config checksum stayed identical.
  These native calls exercise the hide notification, not physical finger input.

Curated [evidence](artifacts/2026-09-26-followup/t604/) includes the red/green
logs, function coverage, native checks and hashes. Private source snapshots,
coverage data, build logs, previous/new AppImages and deployment recipe are
saved under `~/.local/share/blent/profiles/2026-09-26-followup/t604/`.
