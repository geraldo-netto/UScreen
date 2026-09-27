# T705: Linux ADB route ownership

Linux attachment now uses the same `Routes` ownership implementation as the
portable USB connection. Preparation checks the complete route snapshot before
creating missing mappings with `--no-rebind`. Existing matching mappings remain
externally owned; conflicting or malformed snapshots fail closed. Periodic repair
records newly created routes in the same owner. Retirement only removes recorded
routes still matching their original destinations.

A shared owner survives partial preparation and cancelled mutation jobs. The
monitor joins those jobs before retiring routes and reusing the slot. Offline
cleanup remains bounded by the native command deadline and releases capacity;
it can leave a device-side mapping when ADB is unavailable. Revoked credentials
reject the old session; tokenless mode does not provide that authentication
guarantee. ADB snapshot matching is not an atomic compare-and-remove operation
against independently racing external ADB commands.

The shared route constructor admits validated Linux USB, network and mDNS
serials. The existing USB connection constructor retains its stricter USB-only
admission. No Windows native adapter or preview capability was enabled.

Permanent normal-suite T705 regressions were added before the fix. Three failed:
initial conflicting mappings were overwritten, external/replaced mappings were
removed at shutdown, and partial startup attempted to remove an unowned route.
They pass unchanged in behavior after the fix, alongside repair ownership,
offline retirement/stale-lease rejection and bounded invalid-serial/port tests.

Existing T444 fixtures now implement real per-device reverse listings. The
cancellation fixture allows its first route to finish and stalls the second;
it requires first-route retirement before replacement and forbids removing the
uncreated second route. Existing credential rotation, same-device Wi-Fi migration,
launch/backoff and discovery checks remain in the suite.

Validation:

- Fresh instrumented Linux run: 204 library and 390 host-binary tests passed;
  three existing benchmark tests ignored.
- Linux lifecycle, portable USB and Wi-Fi CLI integration: 2 + 1 + 3 passed.
- All 45 production functions across `host/src/monitor*` and
  `host/src/usb/routes.rs` individually meet 80% executable-line coverage.
  The changed `TabletConnection::prepare` in `linux_main.rs` is 21/21 lines.
  The retained broader main-file report includes uncovered entry points because
  this collection ran library/binary unit suites; it is not a whole-file or
  whole-project coverage pass.
- Complexity: 6,435 functions, none above nine. Source formatting and whitespace
  checks pass. Native Windows and physical tablet acceptance were not run.

[Evidence](artifacts/2026-09-27-linux-route-ownership/) includes red/green logs,
integration results, per-function reports, source fingerprints and compressed
raw LCOV. Collection used a fresh LLVM target; a test-only compilation correction
reused that target without changing production sources. Log trailing whitespace
is normalized. Coverage reports were checked before the next installer change.
