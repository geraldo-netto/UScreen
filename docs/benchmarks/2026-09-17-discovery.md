# Independent tablet discovery (T390)

A stalled device no longer holds up every ready tablet's discovery or recovery.
The monitor publishes each bounded device job as it completes, while preserving
per-device forwarding order, retry backoff, USB preference, attachment epochs and
owned teardown. This is a control-plane change, not a video throughput result.

## Reproduction and evidence

The permanent `discovery_tests` use fake ADB shell executables and actual monitor,
attachment, session runtime, listeners and session-ledger code. They run in an
isolated subprocess with temporary HOME/runtime/config directories. No helper,
encoder, uinput device, real ADB server, tablet or desktop is used. The unchanged
slow-recovery and initial-discovery tests failed before the fix: FAST did not
complete within 250 ms while SLOW remained gated. Both pass after the fix.

The scaling test supplies one indefinitely stalled identity probe alongside one,
two or four ready devices. It checks all ready slots, the exact ordered video and
input reverse commands for each device, shutdown within 500 ms, and retirement
of the stalled command while its gate is still closed. Separate contracts cover
bounded task concurrency, duplicate device work, queued cancellation and
promotion of an extra tablet to primary without reusing its retiring slot.
Existing per-device launch/backoff counts remain tested; cross-device launch
order is deliberately independent.

Run in the project's isolated Rust test environment:

```bash
cargo test --locked -p uscreen --bin uscreen t390_ -- --nocapture
```

Five repetitions of
`t390_one_two_four_tablets_connect_and_stop_while_probe_is_stalled` produced the
[raw log](2026-09-17-discovery/trials.log) and
[results with source hashes](2026-09-17-discovery/results.json).
[SHA256SUMS](2026-09-17-discovery/SHA256SUMS) covers those artifacts. The candidate
was based on `ff6fd1a`; measurements used the Rust 1.90 debug test binary in
`uscreen-ci:perf` on the Ryzen 9 7945HX Linux host. Other validation shared the
host during these short trials, so these are observed local completion times,
not stable hardware performance percentiles or a speedup estimate.

| Ready tablets + one stalled probe | Connect median / maximum, ms | Stop median / maximum, ms |
|---|---:|---:|
| 1 | 12.442 / 18.210 | 0.264 / 0.813 |
| 2 | 12.367 / 31.416 | 0.518 / 19.380 |
| 4 | 18.257 / 32.821 | 0.865 / 2.351 |

Connect time starts before spawning the monitor and ends when its ledger lists
all ready devices; 5 ms observation polling is included. Stop time ends when the
monitor joins its owned work, before the fixture's subsequent descendant-reaping
check. The old monitor's initial discovery waited on the gated batch, so the
regression comparison is a failed deadline, not a comparable completed timing.

## Limits and policy

Discovery permits four probe sequences at a time and device mutations permit
four sequences, one per serial. One additional global inventory/reconnect job
runs independently. Queue depth follows the ADB inventory; active display slots
remain capped at four. The two reverse commands and launch within one serial
remain sequential. This monitor serializes its own global ADB operations, not
other processes or manual CLI commands.

Slow or failed forwarding uses the existing per-device exponential backoff.
Retiring extra sessions reserve their slot until shutdown joins; unrelated slots
continue. Known identities retain stable selection and USB/Wi-Fi deduplication.
An absent transport invalidates its observation; late results cannot update a
new observation epoch. Unknown identity and observed absence still require fresh
geometry under T281's rules.

These fixtures cannot establish remote ADB cancellation, real USB/Wi-Fi latency,
video capacity or energy gains. Killing a local ADB client cannot roll back work
already accepted remotely. T413 separately tracks a possible reconnect admission
race after a transient package-query failure; the current boolean package probe
still treats command failure as ineligible. T391 measures media resource limits
separately before any increase to the four-tablet cap.
