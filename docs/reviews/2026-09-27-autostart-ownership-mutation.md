# Windows autostart assertion sensitivity (T658)

Four meaningful native Windows mutants survived the original normal suite:
refusing to create an absent registration key, accepting commands beyond 260
UTF-16 units, rejecting the exact limit, and accepting either an absolute path
or the Blent basename instead of requiring both.

Permanent `t658_enable_creates_absent_registration_key` and
`t658_ownership_requires_absolute_blent_path_with_bounded_command` now exercise
first-install registration, lengths 259/260/261/262/512, relative Blent paths,
root-relative paths and an absolute foreign executable. Registry writes use
private fixture keys. Production behavior remains unchanged.

The native ordinary-user rerun passed its unmodified baseline and caught all
four previously surviving faults. The eight selected mutations at the relevant
source locations were all caught. This is scoped evidence, not a whole-project
mutation score. Other Windows campaign survivors remain part of T652.

[Retained evidence](artifacts/2026-09-27-mutation-resume/t658/) contains selected
before/after outcomes and their baseline, logs, diffs, native identity and exact
production/test source hashes. These rows were extracted from complete 41-mutant
campaigns; the subset does not claim to be a separate campaign.
