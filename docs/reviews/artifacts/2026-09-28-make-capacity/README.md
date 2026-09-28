<!-- Copyright (c) 2026 Geraldo Netto -->

# T711: preserve Make setup capacity

`make setup-system` previously used `tee` unconditionally on
`/etc/modprobe.d/blent-evdi.conf`. The permanent normal Rust tooling regressions
first failed on a configured one-device count and on a managed symlink target.
The target now creates the default only for an absent path, preserves symlinks
including dangling ones, and uses shell noclobber when creating the file.

The same tests pass with counts 1–4, extra module options/comments, empty
configuration, invalid/out-of-range counts and both managed-link cases. Existing
minimal-install coverage also verifies the exact two-device default when absent.
No production function was introduced or changed; the changed Make recipe is
executed directly by these sandboxed integration tests. They do not run sudo,
load host modules or modify live configuration.

Commands:

```sh
cargo test --locked -p blent --test tooling t711_   # red before the fix
cargo test --locked -p blent --test tooling make_setup  # same tests plus fresh install
python3 -m unittest discover -s scripts/tests -p 'test_installer*.py'
```

`red.log` and `green.log` retain the before/after assertions. All 42 installer
regressions also pass. The complexity gate and formatting check passed. Original
logs are retained under `/backups/disk2/blent-work/t711/`; trailing whitespace
was removed from these copies.
