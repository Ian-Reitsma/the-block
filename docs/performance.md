# Performance

Tracks benchmarks and profiling for throughput and latency.

## Parallel Runtime

`node/src/parallel.rs` partitions read/write sets and executes non-overlapping transactions with Rayon. `node/benches/parallel_runtime.rs` measures speedups versus sequential execution.

## Bench Harness

`tools/bench-harness` deploys multi-node clusters and runs configurable workload mixes, generating regression reports.

## GPU Acceleration

GPU-backed hash workloads live under `node/src/compute_market/workloads/gpu.rs`. Tests ensure CPU/GPU determinism across hardware.

Progress: 30%
