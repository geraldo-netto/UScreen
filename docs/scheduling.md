# CPU scheduling

Blent requests **High** CPU scheduling priority by default. Existing TOML
files inherit this preference when the field is absent. Select **General → CPU
scheduling → Normal** to opt out, or set this top-level configuration field:

```toml
scheduling_priority = "normal" # default: "high"
```

Apply the setting to restart the daemon and its capture/encoder children.
Reopen the settings window to apply its own changed priority. Denied requests
are reported in the daemon journal or GUI stderr, and startup continues with
the OS's effective settings. A configured preference is not proof of elevation.
The General tab also shows whether this GUI process's startup request succeeded.

| Backend | High request | Effective scope and limitations |
| --- | --- | --- |
| Linux | cgroup v2 `CPUWeight=1000`, versus Normal `100` | The daemon, EVDI userspace helper, FFmpeg, camera helper, all their threads and new children share the daemon's group. Manually launched daemon/GUI processes get private user scopes. Requires a working systemd user manager, CPU controller, `systemctl` and `busctl`; ordinary user permissions suffice on the tested host. |
| Windows | `HIGH_PRIORITY_CLASS`, versus `NORMAL_PRIORITY_CLASS` | Process threads, plus explicit creation flags for commands launched through the shared bounded-command and reaped-child adapters. Windows does **not** automatically inherit High for child processes. Cross-compiled; native acceptance remains T583. Windows display/ADB support is still unfinished. |
| macOS | process nice `-5`, versus `0` | Standalone adapter, inherited by new children. The OS can deny elevation without privilege; the adapter verifies or reports denial. Cross-compiled, with native validation pending T583. This does not implement the missing macOS application backend. |

Linux's already-running shared ADB server is handled separately: match the
known ADB executables in PATH, user ID and default server arguments, recheck its
process identity, then change only that server's scope. The editor or terminal
that originally launched it is not boosted. This includes a system SDK server
shadowed by AppImage's bundled executable. Other ADB clients also benefit
because this is a shared server. Custom server endpoints are not adopted.
The ADB scope survives a Blent stop; a later Normal startup lowers its weight.

CPU weight is a relative share among competing sibling groups, not a CPU quota,
reserved core, deadline, negative nice value or multiplier of video speed.
Blent/EVDI/FFmpeg share a budget, not a new frame synchronization mechanism.
GPU execution, kernel driver work, Xorg, Chrome and Bluetooth scheduling are
outside this setting. PipeWire's existing real-time audio threads retain their
scheduling class; Blent requests no real-time policy.

Android's synchronous render thread already requests `Thread.MAX_PRIORITY` in
`DecoderConfiguration`/`DecoderSession`. This change does not elevate vendor
codec services, alter Android process importance or replace foreground-service
and power-management rules.

See the [local comparison](benchmarks/2026-09-21-scheduling/README.md). It verifies
greater CPU share under synthetic contention, but shows no consistent live
video-latency improvement and does not reproduce the reported audio stutter.

## Live Linux adjustment and rollback

For an already-running service, this takes effect without restarting EVDI or
Android and persists across service restarts:

```sh
systemctl --user set-property blent.service CPUWeight=1000
```

Use `CPUWeight=100` for the immediate rollback. With the updated application,
also save `scheduling_priority = "normal"` so its next startup preserves that
choice. The daemon's runtime preference overrides the packaged service default.
Inspect `systemctl --user show blent.service -p CPUWeight -p ControlGroup`
and the corresponding cgroup `cpu.weight` for effective state. Separate GUI/ADB
scopes are named `blent-priority-<PID>.scope`; lower their CPUWeight to 100 to
roll back a live experiment. Do not stop these scopes merely to reset priority:
stopping a scope can terminate its processes.

The live experiment applied only scheduler settings to the installed binaries;
it did not reinstall the application or restart the display. The new default,
automatic GUI/ADB scope setup and UI control are available in builds containing
T582. An old binary cannot apply the new TOML setting.

Sources: [Linux CPU controller](https://www.kernel.org/doc/html/v6.12/admin-guide/cgroup-v2.html),
[Windows priority inheritance](https://learn.microsoft.com/en-us/windows/win32/procthread/scheduling-priorities),
[Darwin setpriority](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setpriority.2.html).
