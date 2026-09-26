# Importing native Windows coverage (T636)

The first native LCOV report used relative Windows separators, which the Linux
reporter treated as literal filename characters. New failing regressions cover
mixed separators, mapped drive/UNC roots, unmapped or drive-relative paths,
traversal and retention of zero counters. The importer now maps portable report
paths before enforcing the existing source-tree boundary.

The Windows workflow disables Git's automatic CRLF conversion before checkout,
then retains exact SHA-256 source fingerprints with the native counters. The
fingerprint check remains byte-exact: changing only LF to CRLF still fails its
permanent regression. The collector does not silently accept changed source
bytes to obtain a passing result.

The complete reporting suite passes 62 tests, and the complexity gate reports
5,991 functions with none above 9. Broader native coverage remains T497; the
runtime convenience functions and some child-resumption error paths still need
coverage before T493's complete foundation acceptance can close.

[Native run 36263320613](https://github.com/geraldo-netto/UScreen/actions/runs/36263320613)
at `47efa7a` passed both Windows jobs, including the full native workspace suite,
all-features MSVC build, GNU linking and native common-library collection.
All **54 source fingerprints exactly match** the reporting checkout after the
LF-preserving checkout change. Raw Windows LCOV paths now import successfully.

- [Exact source fingerprints](artifacts/2026-09-26-windows-development/windows-sources.json)
- [Reporter regression failures before the fix](artifacts/2026-09-26-windows-development/t636-red.log)
- [Complete reporter suite after the fix](artifacts/2026-09-26-windows-development/t636-green.log)
- [Complexity gate](artifacts/2026-09-26-windows-development/t636-complexity.log)
- [Native validation output](artifacts/2026-09-26-windows-development/native-validation.log)
