# T419 renderer evidence

See the [report](../2026-09-19-rectangle-renderer.md) for conclusions and limits.
`SHA256SUMS` covers this directory's files. `evidence.tar.gz` contains its own
manifest and original observations, including whitespace in `dumpsys` output.

| Archive directory | Completed trials | Purpose |
| --- | ---: | --- |
| `matrix` | 45 | Five scenes × three paths × three rounds; 20 s + 4 s warmup |
| `presentation` | 9 | Motion with SurfaceFlinger collection; 12 s + 4 s warmup |
| `idle-control` | 3 | H.264 at one update/s; 20 s + 4 s warmup |
| `mmap` | 4 | Motion/photo × LZ4/Zstd, one trial each; 12 s + 4 s warmup |
| `lifecycle` | 12 | Same-process rectangle retirement; 2 s, no warmup |
| `verify-bounded` | 10 | Separate GPU texture verification; 980 readbacks |

Each cohort records the measured APK hash and device fingerprint. Timing
summaries outside the archive are reproduced by
`scripts/benchmarks/summarize-rect.py` from the corresponding extracted folder.
Verify the expected counts above; the script also supports partial cohorts.
The lifecycle summaries include **retired** memory snapshots and are not
active-memory or warmed performance comparators. Cohort PSS percentiles use
the script's floor-rank definition; the report's main memory table instead
uses the ordinary median of all six snapshots, averaging its two middle values.

`main-matrix-sources` preserves the builder/collector snapshot, exact copied
Kotlin, native source and APK provenance when the main matrix started. Later
collector controls and stricter native-source validation appear in
`final-harness`; these changes did not rebuild or replace the measured APK.
The current repository scripts are the maintained reproduction entry points.
`validation` contains five JVM test results, Python/complexity checks, and the
native source-file admission regression's red/green logs.

`fixture-identities` preserves generator metadata, compressed/source RGB hashes,
H.264 quality commands/results, the idle-control derivation and the 104 KiB
NASA photo input. Metadata identifies the DejaVuSansMono font hash. Photo
credit: NASA Earth Observatory, [Blue Marble 2002](https://science.nasa.gov/resource/blue-marble-2002/).
`completion.json` records host process continuity and return to UScreen after
the temporary test APK was removed.

Large RGB/encoded fixtures and APK/NDK/build outputs are intentionally absent.
Follow the [harness guide](../../../scripts/benchmarks/android-rect/README.md)
to recreate fixtures with pinned inputs, recording any changed output hashes.
Hardware encoder/library changes can change output bytes and invalidate a
claim that new trials used this exact corpus.
