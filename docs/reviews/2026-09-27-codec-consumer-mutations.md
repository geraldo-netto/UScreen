# Include codec diagnostic consumers (T670)

Three apparent shared-video survivors came from incomplete test selection:
constant codec labels and removal of the VP9 encoder arm. Existing
`doctor::tests::t438_codec_report_follows_requested_family_and_legacy_scope`
already checks requested-family names and VP9 behavior. A private copy using the
original video source, without the newly added portable tests, catches all three
mutations through the existing diagnostic tests after a passing baseline.
These were not production codec defects or missing repository-wide assertions.

The profile now includes diagnostic callers. Its permanent selection regression
fails with the diagnostics filter removed and passes with the fix. The portable
tests developed during investigation are retained: they independently check
labels, wire names, muxers, framing, VP9 aliases and nearby unknown inputs.
Production behavior is unchanged.

The complete video campaign with the portable contracts catches 13 of 14
mutations; the remaining edit cannot compile because `Codec` has no `Default`.
It has no survivors or timeouts and matching baseline/mutant commands. The
separate three-candidate original-source run establishes the existing diagnostic
coverage. Fresh LLVM counters pass all five video functions individually at 80%
or higher. All 129 common library and 15 runner tests pass; whole-project
complexity passes all 6,294 functions at nine or below.

[Evidence](artifacts/2026-09-27-mutation-resume/t670/) includes source hashes,
both campaigns, baseline/failure logs, permanent profile red/green evidence and
fresh counters. The earlier incomplete selection is preserved with
[T671](artifacts/2026-09-27-mutation-resume/t671/before-video/).
