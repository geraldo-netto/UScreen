# Private namespace prerequisite in CI (T640)

[Run 36273559974](https://github.com/geraldo-netto/blent/actions/runs/36273559974)
failed the retained `camera::native_tests::t572_isolated_native_devices_and_session_cleanup`
regression before coverage reporting:

```text
T572 requires private mount fixtures: unshare: write failed /proc/self/uid_map: Operation not permitted
```

The disposable Ubuntu runner restricts unprivileged user namespaces through
AppArmor. Its build job now loads a profile allowing the existing `/usr/bin/unshare`
test launcher to create user namespaces, then requires a real user/mount/PID
namespace preflight before compilation and collection. The system-wide restriction
is not disabled. The profile is a CI fixture, not an installed Blent policy.
[Ubuntu documents this per-application allowance](https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces).

The T572 native regression remains unchanged and mandatory; private fixtures do
not fall back to real devices. The permanent T640 workflow regression first failed
for the missing namespace prerequisite, then passed after the configuration fix.
Production source bytes and application coverage counters are unchanged.
