# T709: retain provisioned EVDI boot configuration

Source/tarball reinstallation preserves an existing `blent-evdi.conf` instead
of overwriting it with `initial_device_count=2`. This retains the one-to-four
device count previously provisioned through GUI setup, administrator options,
comments and GPU load-order directives. An absent file still gets the established
two-device default; live-device provisioning remains additive and unchanged.

The privileged write checks both existing paths and dangling symlinks and uses
shell noclobber when creating a new file. Existing empty, invalid or managed
configuration remains untouched and may require manual correction; the installer
does not guess replacement settings. This does not resolve T700's boot-ordering
investigation or alter any running EVDI devices.

Permanent isolated regressions were added before changing the installer. Eight
subcases failed across the four tests: capacity/options were overwritten, empty
or unrecognized configurations were replaced, and a dangling managed link was
followed. The same tests now pass; fresh-install default behavior also passes.
Existing boot-write failure and setup-order fixtures were adapted to the new
privileged command while retaining their error/sequence assertions.

Validation: 27 installer tests, 22 related Make/path/autostart/build/EVDI tests
and four targeted tooling integration tests pass. Native Bash counters cover all
38 installer functions individually at least 80%; `configure_boot_modules` is
7/7 executable lines. No exclusions or thresholds changed. Complexity: 6,440
functions, none above nine. All privileged operations in the new fixtures are
redirected to temporary directories; no actual module, system file or tablet
was changed.

[Evidence](artifacts/2026-09-27-installer-capacity/) retains failing/passing logs,
the per-function report, source fingerprints and raw shell trace/origin records.
Coverage was checked before subsequent configuration-merge source edits.
Log trailing whitespace is normalized.
