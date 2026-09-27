# T650 inventory validation

Before the fix, `adb_inventory::parse` accepted an embedded NUL as a ready
serial. Native process arguments reject NUL; a malformed observation therefore
retired the existing USB attachment and its authentication lease.

Both permanent regressions failed before the fix (`red.log`) and pass after it
(`green.log`). They cover every NUL position in a bounded 64-character serial,
valid Unicode, and an existing authenticated socket retaining its token and
responding to ping after malformed inventory. The shared parser now returns
unknown for the entire observation.

Fresh targeted collections pass 197 Linux tests and 87 native Windows tests.
The Windows collection runs as ordinary unelevated `blentdev`. All five functions
in the changed parser file meet 80% independently on both platforms.

`provenance.json` records the three changed source files: one production parser
and two regression files. The other 424 source fingerprints remain identical to
the T525 collection. The baseline LCOV files here remove every record for those
three changed files; fresh Linux/Windows counters replace them. The combined
application gate passes 1,425/1,425 functions with current native fingerprints.
No stale counters for the changed parser contribute to the result.
