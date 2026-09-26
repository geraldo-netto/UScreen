# T633 native development VM evidence

`summary.json` records final native acceptance and source revision. `source-hashes.json`
identifies the retained helper/test bytes. `SHA256SUMS` covers this evidence set.
Application tests use revision `9330d93a295cbe348c253f82caf105a16333775f`.

- `portable-tests.log.gz`: 47 passed; command exit zero.
- `workspace-tests.log.gz`: 221 passed; command exit zero.
- `workspace-build.log.gz`: all-features build, exit zero.
- `ordinary-lifecycle.log.gz` and `ordinary-check.json`: the existing seven
  lifecycle regressions repeated with a limited user token; all passed.
- `activation-check.json`: ordinary-user Xbox activation denied with
  `0x800704EC` and no process. Event 8022 was also observed during acceptance.
  This snapshot predates the final per-user Search setting; use final acceptance
  for the current configuration.
- `acceptance-final.json` and `tools-final.json`: current native configuration
  and tool versions. Final acceptance follows a real stop/start with automatic
  login enabled, as explicitly requested. The clock offset is one second.
- `minimal-policy-{red,green}.json*`: sibling registry value preservation.
- `minimal-publisher-native-{red,green}.json*`: actual Windows AppLocker package
  decisions, including publisher identity, exact product names and versions.
- `minimal-publisher-merge-{red,green}.json*`: replacement of stale owned rule
  IDs while preserving unrelated policy and idempotency.
- `native-stderr-{red,green}.json*`: native command diagnostics must not turn
  successful process exit into a PowerShell failure.
- `status-python-{red,green}.log`: permanent lost-reply regression and the
  bounded invalid-output suite. `status-lock-native-{red,green}.json*` provide
  the supporting real QGA disconnect/write reproduction.
- `acceptance-clock-red.json`: unchanged clock assertion failed at -7,200
  seconds before the launcher used `-rtc base=localtime`. The later authorized
  automatic-login preference changed that separate acceptance assertion.
- `reboot-readback.json`: the earlier restart proved clock correction and the
  then-current disabled-login setting. This is historical; the final accepted
  setting is enabled automatic login in `acceptance-final.json`.

Tests remain in the normal Python and native Windows CI suites. The separate
`windows_vm_acceptance.ps1` suite requires this configured VM and a fresh host
Unix timestamp; it never applies policies. Replay instructions are in
[`scripts/dev/windows-vm/README.md`](../../../../scripts/dev/windows-vm/README.md).

Earlier guest timestamps are two hours behind the host because they precede
RTC correction. They must not be used for performance measurements. No VM disk,
installation media, credentials, unattended answer file or password is included.
This does not validate physical GPU/USB/tablet behavior. The separate Linux CI
coverage failure remains T645; the historical Xvfb startup failure remains T644.
