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
