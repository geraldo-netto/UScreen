# Isolate LLVM coverage objects (T638)

The first T529 Linux collection reused `target/llvm-cov-target`. Its LCOV listed
the same `common/src/commands.rs` functions at both old and current locations:
`output_bounded` at lines 42/44 and `CapturedOutput::new` at lines 54/56. Old
instrumented executables contributed stale mappings. The source fingerprint
checked the current file correctly, but could not certify those old objects.
The contaminated report is rejected rather than edited or treated as a gate.

Recollection used a new, empty `CARGO_LLVM_COV_TARGET_DIR` and unchanged production
bytes from `205f1ef`. All 45 functions in the selected command, pipe-encoder and
shared/Linux FFmpeg files pass. The eight false failures disappear without
changing code, tests or uncovered-line handling. The
[comparison](artifacts/2026-09-26-windows-development/t638-comparison.json)
retains the affected measurements and source hashes.

The [repeatable collection commands](../../scripts/coverage/README.md) now allocate
fresh target directories for each coverage build. Windows CI already collects
on fresh runners. Preserve the pre-build source snapshot as well: object isolation
and byte-exact source validation address different causes of invalid evidence.
This fixes collection guidance; the full cross-platform gate remains T497.
