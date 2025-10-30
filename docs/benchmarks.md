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
exporter now acquires a file lock before rewriting the Prometheus snapshot so
concurrent suites append deterministically without clobbering each other’s
measurements. One `benchmark_<name>_seconds` series is emitted per run, keeping
the most recent timing for dashboards and alerts. The monitoring stack ingests
the file via the
`benchmark_ann_soft_intent_verification_seconds` descriptor in
`monitoring/metrics.json`, and the generated Grafana dashboards now include a
**Benchmarks** row plotting the ANN verification latency beside existing gateway
and marketplace panels. This keeps regression history, alerting, and dashboards
fully first party while letting operators compare wallet-scale ANN timings with
live pacing telemetry.
