# T703: preserve independent camera preference edits

Configuration saves now compare nested table leaves against the editor's original
snapshot and apply only those edits to the latest locked configuration. Changing
camera FPS no longer restores an older mirror, rotation or serial setting. The
existing conflict policy remains: the last save that edits a particular field
wins; an unchanged field does not overwrite a more recent save. Optional-field
removal is an edit, and arrays remain atomic values.

The merge remains schema-driven rather than maintaining a second list of camera
fields. Existing sanitization, validation, file locking and publication behavior
are unchanged.

Permanent ConfigStore regressions were added before the fix. Two failed against
the original whole-camera-table replacement: independent mirror/FPS saves and
optional serial changes combined with rotation edits. The same regressions now
pass in both save orders. They compare complete resulting configurations, retain
unchanged preferences and verify the same-field conflict rule. Additional bounded
tests cover nested tables up to depth 32, integer extremes, scalar/table/array
changes, added fields and removal of an already absent key.

Validation includes 134 shared-configuration unit tests, 65 tests without platform
features, 71 GUI tests, two configuration-location tests and 390 host tests
(three existing benchmarks ignored).
Fresh LLVM counters cover both changed functions completely: `merge_edits` 15/15
and the table merge 17/17 executable lines. All 44 production functions in the
model/storage scope individually meet 80%; complexity remains at most nine
across 6,446 functions. Reports are retained with the raw evidence; this is scoped Linux validation,
not fresh native Windows or physical tablet acceptance.

[Evidence](artifacts/2026-09-27-camera-setting-merges/) retains failing/passing
logs, source fingerprints, per-function coverage and raw LCOV. Collections share
one initially fresh target with unchanged production sources. Trailing whitespace
in logs is normalized.
