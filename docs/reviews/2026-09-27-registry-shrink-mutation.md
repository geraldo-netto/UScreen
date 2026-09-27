# Registry values shrinking between reads (T663)

The surviving `Key::read` mutation changes `value.truncate(size / 2)` to
`value.truncate(size * 2)`. It is not an allocation-only equivalent: if a value
shrinks between the size query and data read, the vector must shrink to the
actual number of UTF-16 units. Otherwise stale trailing zeros reject valid data.

Permanent `t663_read_uses_actual_size_when_value_shrinks_after_probe` uses a
thread-local test-only seam immediately before the native data read. It replaces
a 2,047-unit value with lengths 0/1/2/255/1023/2046 and checks the exact resulting
Unicode data. All writes use a uniquely named fixture key, removed before result
assertions. Release builds omit the seam; production behavior is unchanged.

The native ordinary-user unmodified suite passes all 105 library tests. The
specific truncation mutation fails the new regression after a passing baseline.
Fresh native coverage passes all seven maintained registry functions individually
at the existing 80% threshold. Complexity remains at most nine.

[Retained evidence](artifacts/2026-09-27-mutation-resume/t663/) contains the native
normal suite, mutation baseline/failure, diff, user/elevation identity, exact
source fingerprints and raw/scoped native counters.
