# T250 — fork Android identity and signed release gate

The app now builds as `io.github.geraldo_netto.uscreen`. Host discovery,
Activity launch, background credential delivery and doctor capability queries
use that package with the retained `com.uscreen` class namespace. Active
benchmark tools target the fork; independent replay APK identities and
historical measurements remain unchanged. The fork installs alongside upstream
with separate private settings; no automatic uninstall or data transfer occurs.
See [signing and migration](../release-signing.md).

The two bundle builders and publisher check the actual APK using stock Android
SDK tools before accepting it. Publication fails before release API writes if
signature verification fails, its sole certificate differs from the tracked
public certificate, its package/launcher differs, or it is debuggable. Signing
properties can live outside Git through `USCREEN_KEYSTORE_PROPERTIES`; the
provisioned permanent key was reused, with no private material added to Git.

## Validation

Permanent regressions were added first and failed against the old behavior:
Android exposed the upstream package, host discovery/launch targeted upstream,
and the fully intercepted publisher accepted a wrong-certificate fixture.
Those same tests now pass in their normal suites. The verifier adds missing,
duplicate, wrong, oversized and malformed identity-report cases, 512 seeded
invalid-input cases, SDK discovery and tool failure/timeout coverage.

- Linux daemon: 399 passed; three existing ignored hardware-oriented tests.
- Shared configuration/platform suite passed; targeted component test covers
  all four original class names under the fork package.
- Android: 378 tests, zero failures/errors; release and debug builds succeeded.
- Offline publication/metadata: 16 tests; verifier: nine tests; active benchmark
  tooling: 95 tests. The Cargo tooling entry runs the new release regressions.
- Clippy across all targets/features and Rust formatting passed. Complexity:
  4,171 functions scanned, none above nine.
- Every new verifier function and the shared Rust component builder has 100%
  executable-line coverage. The Python file has one unexecuted module-entry
  line (99% overall); this is not a project-wide coverage claim. T497 remains.

The actual locally built release APK passed package, launcher, non-debuggable
and certificate verification. SHA-256:
`c7d963e906b34c3190a66bf69bd8a6c1a3b6d1c1320f4757c8239f0d63fecc87`.
Its signer fingerprint is
`1b34ed115e476f4d178b49f6076cf9ed6cc07d474f9230ec952bc97bbba70400`.
The actual debug APK was rejected for certificate mismatch. These builds were
not installed, published or pushed; no tablet was needed or contacted.

Compressed red/green logs and coverage data are in
[the evidence directory](artifacts/2026-09-19-fork-identity/).
