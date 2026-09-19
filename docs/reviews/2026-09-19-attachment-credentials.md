# T444: credentials belong to logical tablet attachments

Video and input now authenticate against the credential captured with their
accepted attachment lease. A replacement, detach, reused slot or changed physical
identity under the same ADB serial retires the old credential. A proven same-tablet
USB/Wi-Fi migration keeps its key while retiring old sockets. Different slots
mint independent keys even when constructed from the same startup seed.

Input retains its generation-checked controller claim and dispatch. Video polls
its connection future under the generation lock, so invalidation cannot race a
subsequent socket write. Pending polls release the lock; retirement wakes idle
connections and closes authenticated and unfinished old sessions. Already sent
bytes cannot be recalled from the peer or kernel buffers.

Before reuse, the monitor cancels and joins the old owned device operation,
retires any extra session, and attempts both `adb reverse --remove` operations.
The slot and old route remain unavailable during this work. An offline device
can prevent acknowledgement of cleanup; its revoked credential still cannot
access the replacement. **Tokenless mode cannot provide equivalent attachment
isolation.** These mechanisms do not isolate privileged Android software,
authorized ADB hosts, root, or another process running as the same host user.

The new credential is atomically published in the private runtime directory:
`token` for slot 0, `token-N` for additional slots. The synthetic tablet reads its
selected slot's file. A stale file after detach does not represent a live route.
Entropy failure remains closed and retries; publication or required delivery
failure prevents readiness. Initial delivery and authentication retries use the
protected broadcast on ADB stdin, keeping secrets out of process arguments.
T445 still permits only one automatic Activity launch for a fresh attachment;
manual opening and background token delivery retain their existing behavior.

## Permanent regression evidence

Before changing the corresponding behavior, normal automated tests reproduced:

- A late control connection with a retired token received a greeting.
- A late video connection with that token received codec bytes.
- An already authenticated video connection stayed open after replacement.
- Two fake devices reused a slot without either old reverse mapping being removed.
- A changed physical identity under the same serial retained the old lease.
- The synthetic client read slot 0's key for each of slots 1–3.

All now pass. Seventeen T444 Rust tests also cover current-key reconnect,
USB/Wi-Fi migration, slot isolation, retirement of stalled work, failed delivery
and publication, cancellation during forwarding, unexpected worker failure,
late completion, bounded token retries without Activity launches, atomic/private
files, entropy failure, and malformed/truncated/out-of-bounds credential corpora.
The older T281 regression still checks the original accepted-socket boundary;
its fixture now supplies the attachment-owned credential. T445's launch assertions
are retained while its fixture waits for the new cleanup barrier.

Validation on this Linux checkout, with no tablet, live display attachment or
installation changes:

- Instrumented daemon suite: **392 passed**, three existing opt-in tests ignored.
- Optional in-process encoder build: all **17 T444 tests passed**.
- Shared Linux runtime suite: **13 passed** (including token-generation/mutation coverage).
- Synthetic tablet suite: **16 passed**, including its normal Cargo tooling entry.
- Workspace Clippy, all targets/features with warnings denied: passed.
- Formatting wrapper and whitespace checks: passed.
- Complexity gate: **4,125 functions**, none above nine.

Every production function in `attachment.rs`, `attachment/auth.rs` and the new
monitor credential/preparation/retirement modules measured **100% executable-line
coverage** in the daemon run. The new stream attachment adapter and device-task
retirement method also measured 100%; the Python slot-token reader measured
3/3 executable statements. This is a per-function source/LCOV mapping for these
changes, not proof of project-wide 80% compliance or branch coverage. The broader
Rust, C, Android and script coverage/fuzz requirement remains tracked in T497.

[Compressed evidence](2026-09-19-attachment-credentials/) includes red/green test
logs, coverage data and the scoped function inventory. These are local correctness
checks, not tablet battery, latency or throughput measurements.
