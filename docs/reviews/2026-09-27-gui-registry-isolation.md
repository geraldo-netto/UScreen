# Windows GUI mutation isolation (T660)

The previous GUI test selected its private key through `autostart_key`, which
was also a mutation target. A mutation of that helper could bypass the fixture.
The normal child-process fixture now overrides HKCU before calling the GUI
adapter and restores it on drop. The helper is in a separate test-only module,
outside the selected mutation file. Destination assertions query the requested
registration key independently of `autostart_key`.

Permanent `t660_registry_override_contains_writes_and_restores_hkcu` verifies
containment and restoration with uniquely named private keys. A private negative
control removes only the override call: its normal child regression fails with
“registration escaped the process-local registry root”. It only writes its two
private test keys; neither the negative control nor the GUI campaign touches the
production Run key. The unchanged normal native suite passes all 22 tests.

Both GUI `autostart_key` mutations are caught after a passing native baseline.
Fresh native LLVM counters pass all 10 maintained Windows adapter functions at
80% individually. The scoped import retains every counter for that file; the
complete original GUI LCOV is also archived. Production behavior is unchanged;
the extra Windows dependency is test-only.

[Evidence](artifacts/2026-09-27-mutation-resume/t660/) contains ordinary-user
identity, passing baseline, both mutation outcomes/logs/diffs, the isolated
negative-control failure, source fingerprints and fresh native coverage.
