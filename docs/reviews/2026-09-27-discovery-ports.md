# T708: discovery fixture port collisions

The scaling fixture derived its eight-port range from its PID without checking
or reserving the ports. The first simulated tablet does not bind its own primary
listeners in this fixture; the second does. Occupying the second slot therefore
reproduces the observed first-tablet success followed by `wait_sessions` timeout.

The permanent `t708_scaling_fixture_avoids_an_occupied_pid_derived_slot` test was
added first and failed with that timeout. The fixture now asks the OS for a base
port and reserves the complete contiguous range, retrying boundedly on collision.
It releases that reservation immediately before the unchanged production listener
constructors run. This removes the unchecked PID-based allocation; handing ports
to constructors is not an atomic socket transfer. A future competing bind remains
possible and will retain production tracing, the session snapshot and per-device
ADB command progress in failure output.

The same regression passes, as do all eight discovery tests and all 384 host
binary tests (three existing benchmarks ignored). No deadlines, discovery
assertions or production code changed. Complexity: 6,425 functions, none above
nine. Test-only changes require no new production coverage counters.

This establishes a deterministic fixture failure mechanism matching the earlier
two-tablet timeout. The earlier run did not retain socket ownership evidence, so
its exact competing process cannot be reconstructed from this result.

[Red, green, full-suite and complexity logs](artifacts/2026-09-27-discovery-ports/)
retain the results; trailing whitespace is normalized.
