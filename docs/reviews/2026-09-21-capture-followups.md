# Capture follow-ups, 2026-09-21

## T569 — reproducible Android replay build

`decoder-project.py` now copies production `JsonNumbers.kt` alongside
`DecoderSelection.kt`, preserving both the original and compiled source bytes.
Historical revisions where the dependency does not exist keep the existing
optional-source behavior.

Permanent regression: `test_t569_generated_replay_preserves_json_number_dependency`
in `scripts/tests/test_benchmark_decoder.py`. Before the fix it failed with
`T569: replay must include JsonNumbers.kt`; afterward all eight decoder benchmark
and planning tests passed.

A new `/tmp/uscreen-t569-replay` project built with the production Gradle wrapper,
JDK 17 and Android SDK 34: `:app:assembleDebug`, all 33 tasks executed, success.
No manually copied dependencies. APK SHA-256: `89f37c2516caad68de1f8ff98cdb7ce656e13aa329e44c285a5d9f9d4ed5e886`.
This verifies replay construction; it does not measure video latency.

## T570 — shared capture damage histories

Implemented and validated; see [paired measurements and permanent coverage](../benchmarks/2026-09-21-shared-damage/README.md).
T576 records a separately discovered historical benchmark source-copy defect.
