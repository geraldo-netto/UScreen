# T585: Blent application identities

Blent replaces current UScreen branding throughout Rust, Android, Linux
packaging, launchers, services, configuration/runtime paths, environment
variables and active build/benchmark tooling. Android uses application ID
`io.github.geraldo_netto.blent` and namespace `com.blent`; Rust packages are
`blent`, `blent-gui` and `blent-config`. Both endpoints share the updated media
and camera identifiers. Generated monitors use the BLN manufacturer code and
a correctly padded 13-byte Blent name descriptor.

This is a fresh identity with fresh defaults. No old path fallback, command
alias, settings migration or protocol compatibility adapter was added. The
existing protected Android signing key is retained; its key alias is an
external custody identifier and is not the Android application ID.

Original MIT copyright and permission text, DisplayLink header and LGPL text
remain byte-identical. Distribution tests retain notices and matching source
assets. Current documentation removes original UScreen repository hyperlinks;
authorship stays as plain text and required third-party source/license links
remain. Historical measurement artifacts retain their original identities.
The local Git upstream remote was removed. The maintainer's own hosted fork
still exists at `geraldo-netto/UScreen`, so its real URLs and archive root are
retained; no hosted rename or publication was performed.

Permanent T585 identity tests failed before the rename. A new EDID regression
also caught the shorter name's required padding before correction; both now
pass. Existing regressions were retained and updated for the selected fresh
identities, including Android component discovery, package verification,
installation, autostart, protocol framing and source-package layouts.

Validation before the separate allocation change: the default Linux workspace
suite passed 805 tests, with three existing ignored hardware-oriented tests.
Android unit tests, release build and lint passed; all 559 measured production
functions met 80% executable-line coverage. All 153 C capture functions met
80%; complexity checked 5,602 functions with none above nine. Original and
distributed MIT license hashes match. Optional encoder, full Rust/script
coverage and native deployment evidence are recorded with deployment rather
than inferred from these results; existing native Windows/macOS and camera
coverage limitations remain in TODO.md.

[Immutable validation evidence](artifacts/2026-09-26-blent/) includes identity
and EDID failures/passes, Android/C coverage, complexity and Linux test output.
