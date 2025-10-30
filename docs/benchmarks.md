# Benchmark Notes
> **Review (2025-09-25):** Synced Benchmark Notes guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

## Gateway Rate-Limit Filter

| implementation | throughput @1M rps | speedup |
|----------------|-------------------|---------|
| scalar Xor8     | 240 k rps         | 1×      |
| AVX2 Xor8       | 970 k rps         | 4.0×    |

Benchmarks run on an x86_64 AVX2 host with synthetic 1 M rps micro‑burst
workload. NEON results were within 5 % of AVX2 on an M1 target.

## ANN Soft-Intent Verification

`cargo bench -p ad_market --bench ann` exercises
`ann_soft_intent_verification`, verifying encrypted ANN receipts across snapshots
with 128, 512, 2 048, 8 192, and 32 768 buckets while scaling badge lists from
two-dozen entries up to 1 024 unique badges. Each fixture derives fingerprints
and entropy salts via `crypto_suite::hashing::blake3`, mixes optional
wallet-provided entropy into the ANN key/IV derivation, and asserts that
`badge::ann::verify_receipt` accepts both salted and unsalted receipts. Use the
benchmark to size wallet entropy budgets or to profile verification latency when
expanding badge tables—latency grows linearly with bucket count, so operators can
gauge acceptable ANN fan-out before rolling out larger cohorts.

When `TB_BENCH_PROM_PATH=/path/to/metrics.prom` is set, the shared `testkit`
exporter acquires a file lock before rewriting the Prometheus snapshot so
concurrent suites append deterministically without clobbering each other’s
measurements. Each run persists a `benchmark_<name>_seconds` gauge alongside the
`_p50`, `_p90`, and `_p99` percentiles computed from the captured iteration
durations plus a `benchmark_<name>_regression` flag that flips to `1` whenever a
threshold breach is detected. Operators can optionally pin absolute regression
ceilings via
`TB_BENCH_REGRESSION_THRESHOLDS="per_iter=0.015,p90=0.040,p99=0.060"`; every
key maps to the per-iteration average or one of the tracked percentiles. Failed
checks surface both in stdout/stderr and through the exported
`benchmark_<name>_*_regression` gauges, making it trivial to wire alerts straight
into Prometheus or Grafana annotations.

Threshold keys are normalised to lowercase before comparison, so `P50=0.010`
and `p50=0.010` behave identically. Malformed pairs (`p50=abc`, `per_iter=`)
are ignored rather than aborting the run, letting suites tighten thresholds
incrementally without bricking CI when an operator mistypes a value.

Setting `TB_BENCH_HISTORY_PATH=/var/lib/the-block/bench_history.csv` instructs
the harness to append timestamped CSV rows with the iteration count, elapsed
seconds, per-iteration average, and the recorded percentiles. History pruning is
first-party as well: `TB_BENCH_HISTORY_LIMIT=200` keeps the most recent 200 rows
in the file, and `TB_BENCH_ALERT_PATH` points at an optional text file that will
be atomically overwritten with a human-readable regression summary whenever any
threshold trips. Dashboards ingest both the Prometheus snapshot and the rolling
CSV, letting ANN percentile trends sit beside gateway, pacing, and committee
panels without relying on third-party tooling.
