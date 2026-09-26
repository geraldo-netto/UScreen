# T624 — one ordinary Blent launcher

The profile manifest removes the inherited launcher filter. Both disposable
variants use that manifest: debuggable `profile` and release-optimized
`optimized`. The production ID and signing configuration are unchanged.
[Temporary profiling sessions](../profiling-android.md) reject production or
launcher-bearing APKs and existing test installations, then own installation
and cleanup across workload success, failure and handled interruption.

`ProfileLauncherTest.t624_onlyProductionHasLauncher` failed against the old merged
profile manifest, then passed for debug, profile and optimized variants. Six
Python lifecycle regressions pass, including failed installation and pending
workload cancellation. Existing identity and release-APK suites pass (14 tests).
No production function changed; benchmark helpers are exempt from the production
coverage threshold. These tests are permanent members of their normal suites.

Native checks installed each new APK on the attached RugKing tablet. Neither
appeared in the launcher. The runner removed profile after a successful workload
and optimized after an intentionally failed workload; the production APK path
was unchanged and its single launcher remained. No camera or display session was
started. [Red/green and native evidence](artifacts/2026-09-26-task-batch/t624/).

The focused Gradle commands select `ProfileLauncherTest` separately for each test
task. An earlier command incorrectly applied `--tests` only to its last task;
that unfiltered profiling run was stopped and is not reported as passing.
