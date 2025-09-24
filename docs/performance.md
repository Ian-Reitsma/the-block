# Performance
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Tracks benchmarks and profiling for throughput and latency.

## Parallel Runtime

`node/src/parallel.rs` partitions read/write sets and executes non-overlapping transactions with Rayon. `node/benches/parallel_runtime.rs` measures speedups versus sequential execution.

## Bench Harness

`tools/bench-harness` deploys multi-node clusters and runs configurable workload mixes, generating regression reports.

## GPU Acceleration

GPU-backed hash workloads live under `node/src/compute_market/workloads/gpu.rs`. Tests ensure CPU/GPU determinism across hardware.

## Recent Optimizations

- `--profiling` flag enables `pprof` sampling and writes Chrome trace events to `trace.json` for deep analysis.
- LRU cache in `verify_signed_tx` avoids redundant Ed25519 checks, reusing thread-local buffers to reduce heap churn.
- `BytesMut` serialization in `net/turbine.rs` eliminates temporary allocations when hashing broadcast messages.
- PoW miner caches computed difficulty targets to skip per-iteration division.
- Benchmarks (`cargo bench --bench order_book`) measure order-book placement to track heap fragmentation.
- Replaced `std::sync::Mutex` with `parking_lot::Mutex` in scheduler and peer sets to reduce lock contention.
- Cache-friendly RocksDB prefix cache reduces repeated disk lookups.
- `auto-profile` harness (`cargo run -p auto-profile`) produces flamegraphs for critical paths.
- `--auto-tune` flag runs the profiler suite and suggests thread counts and block limits based on observed hardware.

Progress: 30%