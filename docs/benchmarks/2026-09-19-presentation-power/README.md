# T419 presentation and battery evidence

See the [report](../2026-09-19-presentation-power.md) for conclusions and
measurement boundaries. `SHA256SUMS` covers these files; `evidence.tar.gz`
contains a second manifest covering the extracted observations and sources.

| Archive directory | Contents |
| --- | --- |
| `presentation` | Three replay/Perfetto pairs, exact tracing configuration, processor identity and two complete diagnoses; trace 01 is excluded because it started late |
| `battery` | Ten eight-minute phases, native battery traces, replay results, process samples, memory/thermal observations, exact commands, interpretation plan and frozen collector hashes |
| `power-apk-sources` | Copied Kotlin sources, originals and APK/native provenance for the long-duration probe |
| `harness` | Builder, collectors, analysis/plotting scripts and rectangle source snapshots |
| `validation` | Python and JVM results, complexity check, APK build log and permanent research tests |

The battery matrix uses one APK throughout, SHA-256
`9cbf68a25f92e9cbc5263f743534386cf0b23ba3cd02126ed58ed66a5013f584`.
Presentation diagnosis uses the earlier APK identified in each replay's
metadata. Fixture identities and generation inputs remain in the
[renderer evidence](../2026-09-19-rectangle-renderer/README.md).
APKs, large fixtures, build caches and tool binaries are not bundled.

`battery-summary.json` includes raw converted counter samples and the stable
window calculations. Reproduce it after extraction with:

```sh
python3 scripts/benchmarks/rect-power-report.py /path/to/evidence/battery \
  --processor /path/to/trace_processor_shell
python3 scripts/benchmarks/plot-rect-power.py /path/to/evidence/battery/summary.json \
  --output /tmp/uscreen-battery-plots
```

For each presentation diagnosis, pass the corresponding `trace-02.pftrace`
or `trace-03.pftrace` and its `replay-02/000-motion-60-0` or
`replay-03/000-motion-60-0` trial folder to `rect-trace-report.py`. Preserve the
exact layer and clock checks. The processor version/hash is recorded in
`presentation/trace-toolchain.json`; do not interpret exploratory CSV files,
which include other layers, as the exact-layer diagnosis.

Each battery phase's `replay/summary.json` is independently reproduced by
`summarize-rect.py`. Its shared-service CPU intervals differ from app timing
and battery intervals. Neither sampled memory nor callback counts certify
peak owned memory or physical display throughput.

The interpretation plan discloses its timing: it was recorded after the first
phase finished but before its battery trace was analyzed. All completed
phases are retained, including the unfavorable RGB repeat. Positive signed
current means net charging with USB connected, not measured USB input power.

`completion.json` records probe removal, return to UScreen and host process
continuity after collection.
