# TODO

| id | status | severity | effort | description |
|---|---|---|---|---|
| T211 | open | low | small | Refactor `scripts/tests/test_packages.py::test_t101_container_and_rpmbuild_failures_reject_stale_assets` (baseline line 13): SonarQube cyclomatic complexity 11 exceeds 9. Extract focused responsibilities and reuse compatible helpers; preserve behavior and permanent regression coverage. Verify every resulting function scores at most 9. |
| T214 | open | medium | medium | Refactor `scripts/install.sh::install_deps` (baseline line 18): approved Shell cyclomatic count 24 exceeds 9. Split independent responsibilities and share compatible helpers; preserve behavior and test coverage. Verify resulting functions score at most 9. |
| T215 | open | medium | medium | Refactor `scripts/install.sh::install_files` (baseline line 144): approved Shell cyclomatic count 15 exceeds 9. Split independent responsibilities and share compatible helpers; preserve behavior and test coverage. Verify resulting functions score at most 9. |
| T216 | open | medium | medium | Refactor `scripts/install.sh::system_setup` (baseline line 193): approved Shell cyclomatic count 17 exceeds 9. Split independent responsibilities and share compatible helpers; preserve behavior and test coverage. Verify resulting functions score at most 9. |
