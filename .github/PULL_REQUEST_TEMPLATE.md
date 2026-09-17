## Problem and resulting behavior

Issue/TODO ID(s), trigger, and what changes for the user.

## Validation

Commands/results, test names and (when relevant) hardware/desktop/tablet.
For behavioral fixes, show the permanent regression failing before the fix
and passing afterward. For latency, state the measurement boundaries.

- [ ] Findings and contradictions were recorded in TODO.md before fixing; blocked rows identify an actual missing decision, evidence or prerequisite
- [ ] One commit per resolved finding; only its resolved TODO row was removed
- [ ] Behavioral fixes retain permanent issue/TODO-linked regressions in the normal suite (or this is documentation/policy-only)
- [ ] Relevant checks from CONTRIBUTING.md passed; any obstacles and missing coverage remain in TODO.md
- [ ] Functions meet cyclomatic complexity ≤9 and keep distinct responsibilities separate
- [ ] Both ends/tests updated if the app/daemon protocol changed
- [ ] User-visible behavior and known limitations are documented
