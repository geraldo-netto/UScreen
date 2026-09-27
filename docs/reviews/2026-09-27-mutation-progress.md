# Mutation testing progress after reboot recovery

This supersedes the initial [pause checkpoint](2026-09-27-mutation-testing-handoff.md)
for current work. T652 remains in progress; these scoped campaigns do not establish
a whole-project mutation score. No mutation job is still running at this checkpoint.
Completed fixes and features have separate commits. No commits from this recovery
batch have been pushed.

## Completed campaigns

All rows below have passing unmodified baselines. Counts are raw engine outcomes;
equivalent survivors and uncompilable edits are not counted as caught. Campaigns
overlap, so their counts must not be added into a project score.

| Scope | Native platform | Candidates | Caught | Survived | Unviable | Evidence |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| Linux autostart | Linux | 32 | 32 | 0 | 0 | [Full retained campaign](artifacts/2026-09-27-mutation-resume/linux-autostart/) |
| Shared USB and command transport | Linux | 125 | 112 | 1 | 12 | [Corrected T664 evidence](2026-09-27-usb-mutation-final.md) |
| Shared USB and command transport | Windows | 125 | 112 | 1 | 12 | [Corrected T664 evidence](2026-09-27-usb-mutation-final.md) |
| Windows autostart and registry | Windows, ordinary user | 41 | 36 | 2 | 3 | [T663/T666](2026-09-27-native-mutation-runner.md) |
| GUI registry-key selection | Windows, ordinary user | 2 | 2 | 0 | 0 | [T660 raw evidence](artifacts/2026-09-27-mutation-resume/t660/) |
| Shared release, negotiation, idle, encoder-worker, tablet and raw-layout policy | Linux | 247 | 241 | 2 | 4 | [Full retained campaign](artifacts/2026-09-27-mutation-resume/shared-policy/) |
| Shared codec identity and selected consumers | Linux | 14 | 13 | 0 | 1 | [T670 and diagnostic selection proof](2026-09-27-codec-consumer-mutations.md) |
| Rust Android-command builders and host consumers | Linux | 6 | 6 | 0 | 0 | [T671](2026-09-27-mutation-baseline-scope.md) |

These completed campaigns have zero timeouts. The USB survivor substitutes the
same type-inferred default value; Windows survivors XOR disjoint access flags;
idle survivors apply fixed cost shifts that cancel against the baseline. Their
semantic explanations and raw failures remain in the linked reviews.

T664 corrected an unrelated fixture failure previously credited as a kill.
T666 isolated and reran a transient Windows linker failure. T671 now rejects
baseline/mutant command mismatches and explicitly baselines consumer packages.
T670 demonstrated that omitted diagnostic tests, rather than production codec
bugs, caused three apparent survivors. Interrupted exploratory runs are not
completed evidence. The complete corrected runs above take precedence.

## Validation and project identity

All 129 final common-library tests and 15 runner tests pass on Linux. Native
Windows runner validation passed 13 of 14 tests, with its Linux-only descendant
test explicitly skipped. The earlier native common suite passed 105 tests.
Whole-project complexity passes 6,294 functions, none above nine; Rust formatting
and whitespace checks pass. Fresh per-function counters for the changed scopes
are retained with each finding. The previous 2,293-function combined gate is
historical; this checkpoint does not claim a newly collected whole-project gate.

GitHub is now [geraldo-netto/blent](https://github.com/geraldo-netto/blent), still
the same fork. The local origin, release URLs, packaging and archive paths follow
the rename. [COPYRIGHT.md](../../COPYRIGHT.md) attributes original Blent work to
Geraldo Netto, preserves inherited UScreen attribution, and accompanies packages.
The original MIT LICENSE and third-party notices remain unchanged. See the
[rename evidence](2026-09-27-repository-rename.md).

## Remaining T652 work

Continue into the maintained Rust code outside these profiles, resolve the
`include!` discovery gap, and classify files without applicable generated
operators. Integrate Android/Kotlin, capture C and essential Python/Shell source
mutation campaigns with their normal suites. Preserve baseline isolation, bounded
processes, exact source fingerprints and distinct outcomes throughout.

The three new shared-policy/video/command profiles have Linux campaign evidence;
their Windows campaigns remain unmeasured. Extend Windows descendant-process
cleanup validation beyond the completed single-process timeout test. Preserve
existing platform, benchmark/development-tool and declined-performance dispositions.
No physical tablet/display/camera acceptance is implied or newly authorized.

Persistent local resources are under `/backups/disk2/blent-mutation-evidence/`.
`recovery-venv/bin/python` has the coverage/complexity dependencies; the private
RPM runtime is `recovery/rpm-runtime/bin`. Do not prepend that Python environment
for the full script suite, which uses system Xlib. Windows VM helpers remain under
`/home/netto/.local/share/blent-dev/mutation-checkpoint`; serialize guest access.
The existing VM is running. Preserve its disk/TPM and `C:\src\blent`; native
campaigns used private ordinary-user copies under the guest temporary directory.
Do not reset the synchronized source tree or push without a new push request.
