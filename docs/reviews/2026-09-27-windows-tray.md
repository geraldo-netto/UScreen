# T531: Windows notification-area tray

The Windows daemon now presents its producer-owned USB preparation state in a
native notification-area icon. The existing monitor publishes each assignment
snapshot to the CLI/GUI state file and to a watch channel; the tray consumes the
latest watch value without another polling model. The shared presentation/action
policy is independent of Windows APIs, and Linux uses its compatible status labels.

The preview distinguishes starting, missing ADB, waiting, prepared USB and stopping.
Every Windows tooltip/menu keeps display and input unavailable. Settings starts
the sibling GUI or its discovered PATH fallback, with a reaped child. Quit signals
the existing orderly USB/lifecycle shutdown. Stopping rejects stale actions.
The native adapter owns its window class, window, icon, menu and thread. Its
synchronized target is cleared on window destruction, preventing late updates from
using a recycled window handle. Producer closure and owner drop retire resources.
A failed tray startup is reported while the daemon remains controllable through
its CLI; an interactive Explorer session is required for tray acceptance.

The [native contracts](../../host/src/windows_tray/tests.rs) cover current state,
invalid action/icon IDs, Settings success/failure, stale shutdown actions, popup
cancellation, direct Explorer recreation notifications, missing shell entries,
producer exit, removed icon/window identities and unchanged USER/GDI counts after
five start/stop cycles. The real daemon's
[USB integration test](../../host/tests/windows_usb.rs) checks prepared/offline
status and uses tray Quit, then verifies route/token/runtime cleanup. Existing
lifecycle tests retain CLI stop, ownership and error coverage. Shared tests retain
bounded invalid counts/action IDs and coalesced watch updates.

Review reproduced a callback bug before its fix: a directly delivered
`TaskbarCreated` message bypassed the queued-message handler. The permanent
`t531_native_actions_latest_status_shell_restart_and_resource_retirement` test
failed waiting for the deleted icon to return. The fix routes both queued and
direct notifications through the window callback. The same recreation assertion
passes afterward, with an additional direct failed-update check. Retained
[red output](artifacts/2026-09-27-windows-tray/shell-restart-red.log) and
[patch](artifacts/2026-09-27-windows-tray/shell-restart-fix.diff) document the defect.
The test deletes only its own icon; it does not restart the user's Explorer.

Validation used ordinary `BLENT-WIN11\blentdev`, session 1, with Explorer present:
93 library, 4 CLI, 7 lifecycle and 1 USB integration tests passed. Linux passed
3 shared-state and 4 existing native D-Bus tray tests. Windows GNU cross-checking
of all host targets, formatting and whitespace checks passed. The combined scoped
native line gate passes **all 59 production functions**, each at least 80%, with
no missing counters. Windows collection used a fresh target after the callback
change. Linux production sources/counters were unchanged by that Windows-only fix.
This is a changed-scope result, not a recollected whole-project gate.

[Evidence](artifacts/2026-09-27-windows-tray/) retains source fingerprints, native
identity, raw LCOV, normal test logs, collection command and per-function report.
No physical tablet, display driver, native input or GPU acceptance is implied;
those existing TODO dispositions remain in force.

Windows API contracts: [notification-area guidance](https://learn.microsoft.com/en-us/windows/win32/shell/notification-area)
and [Shell_NotifyIconW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw).
