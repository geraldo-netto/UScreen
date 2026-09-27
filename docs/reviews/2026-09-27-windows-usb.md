# Windows USB connection preview (T525)

Windows daemon startup discovers authorized USB devices through a native ADB
executable from PATH. Shared USB policy selects eligible Android installations,
creates missing reverse routes without rebinding existing destinations, rotates
attachment credentials, and delivers credentials through Android shell stdin.
The existing Android application and authenticated control/video protocol remain
unchanged. No host shell interprets executable paths, serials or credentials.

The daemon binds a slot's listeners before forwarding or token delivery. A failed
preparation remains unavailable; successfully created partial routes remain owned
until cleanup succeeds. Disconnect invalidates the attachment before waiting for
ADB cleanup. Cleanup checks the current route snapshot and preserves preexisting
or already-replaced destinations. ADB does not offer an atomic compare-and-remove
operation: independent concurrent changes to the same endpoint are outside that
snapshot guarantee, and command cancellation cannot undo work already delegated
to the ADB server.

`max_tablets` retains its configured 1–4 slots. `video_port`, `input_port` and the
corresponding CLI overrides determine host listeners; Android's reverse endpoints
remain 8890/8891. `auto_launch_app` selects initial activity launch or protected
broadcast delivery. The preview requires `require_token = true`. Polling preserves
an existing attachment when inventory is unknown, repairs missing routes and
retries credential delivery every five seconds. Failed preparation has a five
second retry delay so another eligible device can proceed.

Owner-validated, bounded `sessions.json` snapshots report **prepared USB
assignments**, not proof of capture, input injection or physical tablet acceptance.
The CLI reports each assignment and the GUI reads the same producer-owned state.
The lifecycle readiness record means the daemon owns its runtime; listeners are
opened lazily for eligible devices. Missing ADB leaves the lifecycle available
with an explicit connection diagnostic. Stop permits bounded in-flight preparation
and cleanup across four slots; incomplete cleanup is returned as an error.

Capture and native input remain unavailable. Preview sessions advertise no touch
or pen device and no video expectation, and reject display-mode changes. There is
no virtual display, synthetic input, camera, Wi-Fi setup or hardware-encoder claim.
Physical USB/tablet acceptance remains T522. T526 and T528/T529 supply subsequent
input and display/stream integration.

## Permanent regression evidence

The native daemon regression first failed because the previous Windows daemon
never invoked the isolated ADB fixture. It then passed discovery, authenticated
control, offline retirement, reconnect/token rotation, stale-token rejection,
status publication and graceful route cleanup. Executable paths contain spaces,
Unicode and `&`; tests use a compiled native fixture, never a host-shell substitute.

Shared tests retain failed-command, unauthorized/offline/malformed-inventory,
partial-startup, route ownership, failed-cleanup, bounded token/serial/port input,
two-slot independence and listener-release coverage. Additional regressions first
reproduced a failed device monopolizing retries and the preview accepting an
unavailable display mode; both pass after the fixes. Failed or structurally malformed
listings remain unknown.

Sources: `host/tests/windows_usb.rs`, `host/tests/portable_usb.rs`,
`host/src/usb/tests.rs`, `common/src/windows/lifecycle_tests.rs`, and
`gui/src/status_poll/windows.rs`. Shared discovery and Android command builders
also retain the original Linux regressions.

Validation: **887 default + 757 all-features Linux tests**, **244 native Windows
workspace tests** under ordinary unelevated `blentdev`, and a supplemental native
command fixture. The shared native fixture also reruns the Windows daemon test.
Linux passes **1,325/1,325** function gates; the combined native Linux/Windows
application report passes **1,425/1,425**. The Windows USB/domain/native adapter
scope independently passes **130/130**. Shared control functions use combined
native counters; the new fixed-mode guard is also exercised on Windows.

Each coverage collection used a fresh target directory. The supplemental Linux
fixture closes the sole remaining Linux-native command adapter gap; it changes
no maintained production source or function inventory. The baseline and expanded
manifests retain the exact test-file changes, and Windows source fingerprints
match every current production source. No native counters come from Wine or
cross-compilation. See [counts and provenance](artifacts/2026-09-27-windows-usb/summary.json)
and [retained raw evidence](artifacts/2026-09-27-windows-usb/).
