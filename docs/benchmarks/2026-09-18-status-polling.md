# Desktop status polling — 2026-09-18

T409 reduces repeated status discovery and ties the background worker to its
window's lifetime. These are deterministic source/probe-count measurements, not
claims of faster video, lower tablet power or end-to-end frame latency. Full
process discovery and pinned identity checks remain mandatory for destructive
daemon/capture actions.

## Policy

- GUI status normally refreshes two seconds after each completed sample. The
  daemon PID file is reread every time. A valid cached process is fully revalidated
  by UID, start ticks, native executable path/arguments/cwd and daemon state.
  Changes invalidate that identity and fall back to discovery. A new valid tracked
  PID takes priority over a still-running cached daemon.
- Recovery scans first filter `/proc` entries by owner, then by `comm` for the
  status-only daemon search, before reading full process snapshots. The full
  snapshot rechecks ownership to reject a replacement PID between stages.
  General same-user discovery uses only the UID prefilter and retains every name.
  `daemon::discover`, used by start/stop/restart, still inventories all same-user
  processes and applies the existing authoritative checks.
- Program availability and autostart status have a ten-second cache. Explicit
  UI action completion invalidates it and requests a fresh sample. External
  installations/autostart changes appear on the first sample after expiry.
  Sampling and bounded command time add to that interval; ten seconds is not a
  wall-clock refresh guarantee during slow probes.
- EVDI capacity, uinput access, daemon identity and the live session ledger are
  sampled every cycle. ADB devices are queried only when ADB is available and a
  validated live session ledger has assignments. Without an assignment, the
  existing UI already reports no active tablet, so no ADB query is needed.
  Stale ledgers and unrelated charging phones still cannot claim a tablet.
- Status changes and update-check results request repaint. Idle windows have no
  periodic repaint timer; outstanding save/daemon actions request a repaint
  within 100 ms so their completion is observed. Button actions started during
  the current frame are included.
- The worker owns a bounded, coalescing refresh channel. Dropping the window
  closes it; a pending refresh cannot restart polling after destruction. An
  already executing sample completes under the existing command deadlines.
  Destruction does not synchronously join a blocked external command.

## Controlled probe counts

Permanent normal-suite fixtures provide a fake process inventory, daemon source,
clock, status source and ADB response. Counts below are logical probe calls under
those fixtures. Permission failures and process exits affect real syscall counts;
these counts must not be presented as a corresponding speedup factor.

| Controlled case | Previous work | Selected work |
| --- | ---: | ---: |
| 1,000 process entries, 100 same-user, general discovery | 1,000 full-snapshot attempts | 100 full-snapshot attempts |
| Same inventory, two named daemon candidates | 1,000 full-snapshot attempts | 100 name reads, two full snapshots |
| 30 polls with a valid tracked daemon | 30 inventories | 30 identity inspections, zero inventories |
| 30 dynamic polls at two-second virtual intervals | 30 program/autostart groups | Six program/autostart groups; all 30 dynamic samples retained |
| 30 polls without assigned sessions | 30 ADB device commands | Zero ADB device commands |
| Assigned session with available ADB | One query per poll | One query per poll; model/connection updates retained |
| Idle UI with no status change | One repaint request per second | No periodic repaint request |

The owner-filter fixture deliberately changes one candidate's UID between the
prefilter and full snapshot; it is rejected. It also retains an invalid-UTF-8
native executable path unchanged. The PID cache fixtures cover stale PID files,
PID reuse, executable changes, exit and a new tracked daemon. Existing T260
integration tests still exercise real process identity and action routing.

## Lifetime regressions and validation

The prior detached worker retained its status slot and continued polling after
its owner was dropped. The existing loop was first extracted behind a seam;
`t409_dropping_owner_stops_polling_after_inflight_probe` then failed with the
original behavior. After binding the channel to the owner, a second forced
interleaving exposed a queued-refresh edge case. The permanent
`t409_pending_refresh_cannot_outlive_owner` failed before its fix. Both pass;
neither is skipped or weakened.

The idle-repaint test settles an actual headless egui window, verifies no idle
periodic repaint, then injects pending action completion and checks its message.
Capability tests cover virtual-time expiry and explicit invalidation. The ADB
fixture checks no idle/unavailable-tool query, an assigned tablet/model and a
bounded-command timeout response without stale connection state.

Reproduce with the repository's Rust toolchain:

```sh
cargo test --locked -p uscreen-config -p uscreen-gui
cargo test --locked -p uscreen-config -p uscreen-gui t409
cargo clippy --locked -p uscreen-config -p uscreen-gui --all-targets -- -D warnings
```

These tests use sandboxed processes and fake sources. They do not restart the
installed Linux daemon, Cinnamon or the tablet app. Lifecycle-bound daemon push
notifications were considered; this change retains polling as the recovery path
because it also detects external PID-file/session/setup changes without adding a
new daemon subscription protocol. ADB presence for active assignments still uses
the two-second poll, so no event-notification latency improvement is claimed.


[Retained logs and checksums](2026-09-18-status-polling/) record both failing
lifetime regressions, their final passing cases, probe fixtures, Clippy and
complexity results. The host/common/GUI unit gate passed 248 host tests (one
pre-existing ignored test), 35 common tests and 32 GUI tests. Command-lifetime
integration (six tests) and GUI startup (one test) also passed. After the final
cancellation-state refinement, all six GUI T409 tests passed again. Clippy passed
with warnings denied; no reviewed function exceeded cyclomatic complexity nine.
