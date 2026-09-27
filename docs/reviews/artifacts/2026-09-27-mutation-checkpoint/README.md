# T652 initial mutation evidence

See the [resume handoff](../../2026-09-27-mutation-testing-handoff.md).

- `summary.json`: exact tested functions, omitted parser functions, source hash,
  candidate inventory limitations and incomplete project-wide status.
- `outcomes.json.gz`: successful unmodified baseline and 26 caught mutants.
- `mutants.json.gz`: the 26 tested mutations with source locations and diffs.
- `parser-logs-and-diffs.tar.gz`: native Cargo build/test logs and applied diffs.
- `parser-run.log`: concise cargo-mutants 27.1.0 completion output.
- `rust-inventory.json.gz`: 4,908 generated workspace candidates; listing only.

No project-wide mutation score is claimed. No permanent mutation runner or new
regression tests have been added yet; existing parser tests caught this campaign.
T652 remains open. These artifacts contain evidence, not build/cache directories.
