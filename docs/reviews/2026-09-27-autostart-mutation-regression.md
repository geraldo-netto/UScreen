# Autostart probe assertion sensitivity (T653)

The Linux autostart campaign found two meaningful survivors: replacing `&&`
with `||` in `systemd_available` and `systemd_enabled`. Existing fixtures coupled
successful exit status to expected output, allowing either half of each
requirement to disappear unnoticed. Production behavior was already correct.

Permanent `common/tests/autostart.rs::t653_systemd_probes_require_success_and_exact_state`
executes 44 isolated native subprocess cases: four exit statuses crossed with
eleven replies, including plausible states after failures, wrong states after
success, ASCII whitespace, empty output, embedded NUL and invalid UTF-8.
The private `systemctl` fixture never contacts the real user service manager.

Both source mutants now fail the new regression; its unmodified baseline passes.
All four normal autostart integration tests also pass. Production source is
unchanged, so this closes an assertion gap without changing autostart behavior.
This is evidence for two specific mutants, not a project-wide mutation score.

[Retained evidence](artifacts/2026-09-27-mutation-resume/t653/) includes original
and mutated test logs, source diffs, exact candidates, outcomes and fingerprints.
Reproduce from an isolated source copy containing the permanent test:

```sh
cargo mutants --package blent-config --file common/src/linux/autostart.rs \
  --re 'replace && with \|\| in systemd_(available|enabled)' \
  --cargo-arg=--locked --cargo-arg=--test=autostart --cargo-test-arg=t653_ \
  --timeout 30 --build-timeout 600 --copy-target=false --jobs 2 \
  --jobserver-tasks 4 --output /absolute/new/evidence
```
