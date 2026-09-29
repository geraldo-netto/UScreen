# T722: complete Xvfb display publication

`start_private_server` previously read the display pipe once, accepted any
numeric prefix, and closed the pipe. Xorg's `NotifyParentProcess` writes the
number and newline separately (`os/connection.c` in the locally retained
21.1.12 source). A scheduling gap between those writes let the fixture close the
pipe before the second write. Xvfb then aborted with `Cannot write display number
to fd`; clients saw connection refusal or reset.

The fixture now reads through the newline with one five-second deadline and a
100-byte cap. EOF, invalid bytes/records, oversize publication, and timeout fail
with owned-process diagnostics. Initial X connection failures also preserve
display name, child PID/exit, stderr and available `/proc` state before cleanup.
Visibility assertions and timeout duration are unchanged.

- [Permanent regression red run](regression-red.txt): split publication returned
  `:4` instead of `:42`; incomplete records and the shared deadline were also
  mishandled, and connect failure lost server diagnostics.
- `scripts/tests/test_benchmark_visibility.py` retains six T722 tests, including
  all partitions of boundary numeric records and all 246 non-digit byte values.
  These execute in the normal script suite and Rust's T382 tooling integration.
- [Full benchmark suite](benchmark-green.txt): 120 tests passed in 3.707 seconds.
- [Before](isolated-probe.jsonl.gz): 15 failures in 24 native fixture attempts.
  The child stderr retained the display-pipe write failure.
- [After](isolated-green.jsonl.gz): 24 native attempts, zero failures.
- Scoped project complexity check: 34 functions, maximum cyclomatic score 7.
  This changes test fixtures, not production functions.

The native replay uses two concurrent fixture instances within one bounded job
to reproduce the scheduling condition. It has its own network, mount and PID
namespaces and private `/tmp`; it cannot connect to the host X11 sockets.
Only CPUs 30–31 were available, with nice 19 and idle I/O priority. Existing
namespace-local Xvfb diagnostics include unrelated keymap warnings.

From the repository root on a Linux host permitting unprivileged namespaces:

```sh
nice -n 19 ionice -c3 taskset -c 30,31 timeout --kill-after=2s 45s \
  unshare --user --map-root-user --mount --net --pid --fork --kill-child --mount-proc \
  bash -c 'mount -t tmpfs -o mode=1777,size=64m tmpfs /tmp && exec python3 docs/reviews/artifacts/2026-09-29-t722-publication/native-replay.py'
```

Choose allowed CPU numbers on a different host. Run the benchmark suite in the
same isolation, replacing the Python command with
`python3 -m unittest discover -s scripts/tests -p 'test_benchmark*.py'` and allowing
256 MiB for private `/tmp`. Each workload runs sequentially relative to other
validation. No display manager, native input, EVDI module or desktop preference
was changed. This establishes T722's fixture fault; it does not establish the
cause of T721's physical mouse failure.
