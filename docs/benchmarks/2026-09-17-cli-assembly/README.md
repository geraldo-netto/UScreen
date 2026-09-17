# T407 artifacts

- `results.json.gz`: final series, 1,080 rows, source/executable hashes and environment.
- `summary.json.gz`: median/min/max statistics by profile, codec, workload, sessions and variant.
- `t384-*` and `pre-*`: compressed source counterparts for `1e7b045` and `8305f04`.
- `initial-results.json.gz` and `initial-annex_b.rs.gz`: first valid candidate, before spare-read refinement.
- `refined-precheck-results.json.gz` and `spare-read-results.json.gz`: spare-read candidate, both using `spare-read-annex_b.rs.gz`; the former overlapped a short complexity check and is not the final series.
- `discarded-cache-results.json.gz`: invalid comparison; all variants executed the same cached binary (SHA256 `c6462073826948de204e750a3b427a603670c6e4f588cc3e94967d15128e53f9`). Do not use for comparative claims.
- `build.py.gz`, `run.py.gz`, `summarize.py.gz`: exact build, alternating-run and summary controllers; paths target the isolated `/tmp` exports described in the report. The summary was subsequently compressed without changing its data.
- `red.log`: five unchanged assertions failing before the implementation.
- `suite.log`: earlier complete passing suite; `suite-final.log`: final 219 passing tests plus the explicitly ignored timing harness.
- `clippy.log`: final all-target Clippy with warnings denied.

SHA256SUMS covers compressed files as stored. Source hashes in results refer to
the decompressed source contents. Only the final series supports the report's
numeric comparisons; preliminary data is retained to make refinements auditable.
