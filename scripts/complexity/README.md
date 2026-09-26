# Cyclomatic complexity gate (T381)

From the repository root, using Python 3.12 and JDK 17 (Python 3.12/JDK 21
also work locally):

```sh
python3 -m venv /tmp/blent-complexity-venv
/tmp/blent-complexity-venv/bin/pip install -r scripts/complexity/requirements.txt
/tmp/blent-complexity-venv/bin/python -m unittest discover -s scripts/complexity -p 'test_*.py'
/tmp/blent-complexity-venv/bin/python scripts/complexity/check.py
```

`--verbose` prints every function's source location, name and score. Exit 1
means a score exceeds 9; exit 2 means the audit could not complete. Parse errors
are failures, never permission to skip a file. The manually dispatched CI workflow runs the boundary/rule tests
and the whole-project gate. Pushes and pull requests do not trigger CI.

Python parser dependencies are pinned in `requirements.txt`. Kotlin uses the
1.9.20 compiler's PSI parser, with pinned SHA-256 checksums for its Maven jars
in `kotlin.py`. The first run downloads those jars from Maven Central into
`$XDG_CACHE_HOME/blent-complexity` (default `~/.cache/blent-complexity`).
Subsequent runs reuse verified jars and a checker jar keyed by source hash.
No Sonar implementation is vendored or required at runtime.

## Scope

Git-tracked files and untracked, nonignored files are included, including tests
and this checker. Deleted files, dependency/build/cache directories, generated
Gradle wrappers and the upstream `host/evdi/evdi_lib.h` are excluded explicitly.
Kotlin build scripts, Arch `PKGBUILD`/`.install`, Debian `postinst`, RPM shell
scriptlet functions and Python heredocs passed directly to `python`/`python3`
are included. Embedded Python retains the containing script's line numbers.

Only functions/methods are scored: top-level scripts, Make recipes and RPM
scriptlet command sequences are not implicitly treated as functions. Source
strings containing fixtures or generated code are not analyzed as code.

## Pinned rules and limitations

This is a source audit aligned with the following Sonar cyclomatic rules,
**not a native SonarQube run or cognitive-complexity score**. Recheck rules and
fixtures deliberately when updating a parser or Sonar reference.

| Language | Count and reference |
| --- | --- |
| Rust | Nonempty function bodies, closures, if/loop/while/for, logical operators and nonempty match arms. Nested functions contribute to their containing function too. Follows [Sonar Rust at `9b347aa`](https://github.com/SonarSource/sonar-rust/blob/9b347aa4298afac0460084be13db6671116152ed/analyzer/src/visitors/cyclomatic_complexity.rs). Macro token trees are opaque; macro expansion can hide branches. |
| Kotlin | Named functions with a body, if/loops, each when entry (including else), logical AND/OR. Nested named functions contribute to their container. Follows [Sonar Kotlin at `b10cb30`](https://github.com/SonarSource/sonar-kotlin/blob/b10cb30596e0874f5a7ad10180fafaf6c09e8ef3/sonar-kotlin-metrics/src/main/java/org/sonarsource/kotlin/metrics/CyclomaticComplexityVisitor.kt). PSI parsing does not require type resolution. |
| Python | Function baseline, if (excluding elif), for/while, conditional expressions, each logical operator and comprehension filter. Nested functions are scored separately. Follows [Sonar Python at `0161137`](https://github.com/SonarSource/sonar-python/blob/0161137cf31b6ead71293f5f35d5ced18f55bc0b/python-frontend/src/main/java/org/sonar/python/metrics/ComplexityVisitor.java); this intentionally differs from common generic Python counters. |
| C | Approximation: one plus if/for/while/do, case/default labels, ternaries and logical operators. Source-tree parsing covers conditional-compilation branches but does not expand macros or use Sonar's proprietary C analyzer. Compiler/preprocessor semantics can differ; a passing count is not native CFamily certification. |
| Shell | Maintainer-approved standard: one plus if/elif, loops, case alternatives and short-circuit operators. No native Sonar Shell metric is claimed. RPM macros are not expanded; scriptlets are parsed as shell. |

`test_metrics.py` permanently checks 9/10 boundaries for every language,
selected rule differences, parser rejection, embedded Python/package coverage,
and source exclusion. Rust macro opacity has an explicit fixture so it cannot
be mistaken for expansion-aware coverage. Architecture and responsibility
reviews remain necessary even when every function passes.
