# Git ignore and history audit — T489

Reviewed on 2026-09-18 from local HEAD `491ac6e`, including the existing dirty
working tree. **No confirmed credentials were found, and no history rewrite or
remote mutation was warranted.** This is a credential/file-hygiene audit, not a
claim that every possible secret representation can be detected automatically.

## Scope and evidence

- Inventoried 920 tracked files, 27 visible untracked files and 62,605 ignored
  files. The ignored set consists of build outputs, Android caches, compiled
  Python caches and local instruction files. Cache/build contents were not
  scanned as publishable source.
- Checked the fork's two advertised branches and eleven tags; all referenced
  objects were already available locally. The remote refs stayed unchanged
  during review. No pull-request refs or release assets were advertised by the
  fork at the time of the audit.
- Gitleaks 8.30.1 scanned all reachable Git history: 585 commits and approximately
  8.11 MB of textual changes. The executable came from the official release and
  its SHA-256 matched that release's checksum manifest.
- A separate scan exported **every stored Git object**, including objects not
  reachable from ordinary branches: 3,272 blobs, 637 commits, nine annotated tags
  and 3,194 trees. Blob contents and commit/tag records were scanned; tree entries
  were inventoried. Archive-aware scanning processed approximately 1.38 GB.
- The working-tree scan included tracked and visible untracked files, recursively
  traversing archives to depth eight and decoding supported encodings to depth
  three. It processed approximately 1.33 GB, including retained benchmark data.
- Added temporary exact-match rules for the locally provisioned signing password
  and the current UScreen session token, plus a rule for captured UScreen token
  fields. Neither exact credential nor any captured authentication token matched.
  Scan configuration containing those exact values stayed outside Git in a
  private audit directory and was removed after use.

Two scanner candidates were verified as false positives:

| Candidate | Validation |
| --- | --- |
| `docs/benchmarks/2026-09-18-power-policy/deployment.json:129` | The value equals SHA-256 of `android/app/src/main/java/com/uscreen/TokenActivity.kt` at its introducing commit `118b486`; it is source provenance. |
| Commit `f94cac4`, message line 10 | The matched value is the ordinary phrase `fragmented/partial` in the T267 regression description. |

The newly provisioned private key and password are outside this checkout. The
public release certificate under `docs/` contains public verification material
and is intentionally permitted. Compressed benchmark fixtures, measurement logs,
plots, launcher icons, the Gradle wrapper and `Cargo.lock` are legitimate project
files; they were not removed as generic binary/build output. `.gitignore` cannot
filter sensitive content embedded in a permitted archive or protect an explicitly
force-added file, so retained evidence still needs content review.

## Changes and validation

The previous ignore rules covered JKS/keystore files but missed the PKCS12 format
used by the fork's new key, its password file and private PEM/key files. Added
scoped rules for those credentials, local Python/Kotlin caches, Android bundles,
root AppImage build products, native helper intermediates and JVM crash outputs.
The known public certificate has an explicit exception. Broad `*.log`, `*.gz`,
`*.bin`, `*.jar` and lockfile exclusions were avoided to preserve project evidence
and required source-controlled inputs.

Permanent normal-suite regression
`host/tests/tooling.rs::t489_private_signing_and_local_outputs_stay_out_of_git`
covers 32 private/generated and retained-source paths. It failed before the fix
on `android/release.p12`, then passed with the updated rules. Existing T342
build-output coverage also passed. Targeted Clippy with warnings denied,
formatting and the project complexity check passed. No currently tracked path
became ignored.

The remote audit used read-only `git ls-remote` and GitHub API queries. Since the
candidates were not credentials, history was preserved. If a real exposure is
found later, follow [GitHub's sensitive-data removal procedure](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository):
invalidate exposed credentials, verify a scoped rewrite, and account for copies
or cached references beyond the rewritten branches. The scanner's capabilities
and limits are documented in [Gitleaks](https://github.com/gitleaks/gitleaks).
