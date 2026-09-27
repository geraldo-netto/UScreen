# GitHub repository rename (T661)

The hosted fork is now `geraldo-netto/blent`. GitHub repository ID 1373334703 and
node ID `R_kgDOUdtsrw` match the pre-rename repository, its upstream parent remains
`majmichu1/UScreen`, and its default branch remains `main`. Local origin uses the
new HTTPS URL. This rename preserves the existing repository and history.

Release/update endpoints, issue/star links, packaging metadata, website metadata,
documentation links and the release publisher use the new repository. Arch's
build/package functions and native build fixture use the `blent-$version` archive
root. Historical raw measurement artifacts are retained unchanged.

Permanent T225/T661 publisher tests first failed on the old API/URL destinations;
the retained T497/T661 Arch build regression failed when the old recipe tried to
enter `UScreen-1.2.3`. Both pass with the new identity. The publisher test exercises
isolated fake requests and does not publish a release. All 17 release tests,
Arch source-build tests, Linux GUI tests, Android settings/release tests and
normal packaging/notice suites pass.

Fresh source-attested coverage passes every one of the 52 affected shared Rust
functions, all 593 Android methods, 61 essential Python functions and 60 essential
Shell functions at the existing 80% per-function threshold. Native Windows GUI
adapter isolation and its fresh counters are recorded under T660. The complexity
gate checks 6,266 functions with none above nine.

[Evidence](artifacts/2026-09-27-repository-rename/) retains red/green tests,
repository readback, source snapshot, native Linux/Android counters and scoped
reports. The separate T662 commit adds contribution notices while retaining
original and third-party notices. No commits or releases were pushed by this
rename operation.
