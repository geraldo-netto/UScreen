# T597: algorithmic complexity versus measured cost

Not every operation uses its smallest theoretical asymptotic bound. A concrete
example is latency reporting: `host/src/latency.rs::Report` sorts up to 1,024
samples, O(n log n), then reads p50, p95 and maximum. Order-statistic selection
can produce those values in O(n). That does not establish a practical win for
all inputs, or make the current bounded report a bottleneck.

The retained [standalone Rust benchmark](artifacts/2026-09-26-performance/t597/)
compares the existing sort/index algorithm with two `select_nth_unstable` calls
and a maximum scan. It uses the same nearest-rank rounding, preallocated storage,
20,000 iterations per sample, three trials, sizes 16/64/256/1,024 and random,
eight-value duplicate, and sorted data. Fixture reset is included equally; no
host/tablet display workload ran concurrently. This is an algorithm microbenchmark,
not a production latency or energy measurement. The original measured wrapper is
retained as `percentiles.rs.gz`; the runnable `.rs` splits fixture/sample duties
to respect the cyclomatic limit and adds a reference-result assertion.

Median nanoseconds per report at size 1,024:

- Random: sort 4,426; selection 2,060.
- Eight-value duplicates: sort 2,071; selection 1,499.
- Already sorted: sort 277; selection 1,911.

At size 16, sorting is faster for all three distributions. A roughly two-
microsecond reduction once per five-second window is not an actionable runtime
bottleneck in the measured pipeline. Keep current production behavior; retain
T597 as explicitly deferred until report size/frequency or measured CPU cost
justifies it. The existing tests and capacity reuse remain intact.

`select_nth_unstable` has linear worst-case complexity according to
[the Rust slice API](https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable).
Sorting's adaptive behavior and smaller constants explain why Big-O alone is
insufficient. An optimality claim for every maintained function would also need
input models, contracts and lower bounds; cyclomatic-complexity checks do not
supply that proof. The [main audit](2026-09-26-performance-audit.md) identifies
which pipeline costs scale with pixels, packet bytes, queue depth and inventory
size, and which were actually sampled.
