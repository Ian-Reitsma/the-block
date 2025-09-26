# Parallel Execution and Transaction Scheduling
> **Review (2025-09-25):** Synced Parallel Execution and Transaction Scheduling guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The Block separates conflict detection from execution so that independent
transactions and tasks can run concurrently. Two primitives coordinate this:
`ParallelExecutor` for generic workloads and `TxScheduler` for UTXO
transactions. This guide outlines their semantics and best practices.

## Runtime abstraction

Async helpers across the node, CLI, and auxiliary tooling no longer wire Tokio
directly. Instead the workspace uses [`crates/runtime`](../crates/runtime/) as a
facade that exposes `spawn`, `spawn_blocking`, `sleep`, `interval`, `timeout`,
and `block_on`. The wrapper selects a backend at process start, defaulting to
Tokio but supporting an opt-in stub executor for synchronous tests. The active
backend can be overridden by setting `TB_RUNTIME_BACKEND=tokio` or
`TB_RUNTIME_BACKEND=stub`; unsupported values fall back to the default while
logging a warning.

Crates consuming the abstraction should import the helpers from the facade
(`runtime::spawn`, `the_block::spawn`, etc.) rather than referencing Tokio
handles or timers directly. This keeps scheduling code agnostic to the event
loop implementation and allows future experimentation without invasive changes.
Integration tests under `crates/runtime/tests/` exercise both the Tokio and
stub backends to ensure consistent behaviour.

## 1. `ParallelExecutor`

`node/src/parallel.rs` provides a thread‑pool powered by `rayon`. Callers
construct a list of `Task` objects, each declaring the keys it reads and
writes. `ParallelExecutor::execute()` partitions tasks into non‑conflicting
groups and executes each group in parallel.

```rust
use the_block::parallel::{ParallelExecutor, Task};

let t1 = Task::new(vec!["a".into()], vec!["b".into()], || work1());
let t2 = Task::new(vec!["c".into()], vec!["d".into()], || work2());
let results = ParallelExecutor::execute(vec![t1, t2]);
```

### Conflict Rules

Two tasks conflict if any of the following overlap:

- writes ∩ reads
- reads ∩ writes
- writes ∩ writes

`ParallelExecutor` performs a greedy scan, inserting each task into the first
group where no conflict occurs. This keeps scheduling deterministic and
avoids livelock.

## 2. `TxScheduler`

For on‑chain transactions `TxScheduler` tracks read/write sets derived from
UTXO inputs and outputs. `schedule(tx)` registers a transaction if it does
not conflict with any running transaction; otherwise it returns the
`txid` of the conflicting entry.

```rust
use the_block::scheduler::TxScheduler;

let mut sched = TxScheduler::default();
if let Err(conflict) = sched.schedule(&tx) {
    log::warn!("conflicts with {conflict:?}");
}
// once executed:
sched.complete(&tx);
```

Internally `TxScheduler` stores a `HashMap<txid, TxRwSet>`. `TxRwSet`
collects input `OutPoint`s as reads and generated outputs as writes. Conflict
checks mirror those of `ParallelExecutor`, ensuring exactly‑once semantics for
UTXO spends.

## 3. Best Practices

- **Derive precise read/write sets.** Over‑approximating writes reduces
  parallelism; under‑approximating risks double spends.
- **Call `complete()` promptly.** Leaving transactions marked as running will
  block unrelated work.
- **Batch independent tasks.** `ParallelExecutor` excels when many small
  operations can be grouped with disjoint read/write sets.

## 4. Telemetry and Testing

- `node/tests/parallel_executor.rs` and `node/tests/scheduler_parallel.rs`
  demonstrate conflict scenarios and parallel speedups.
- Benchmarks in `node/benches/parallel_runtime.rs` measure throughput gains.
- Consider instrumenting high‑level schedulers with queue depth and conflict
  counters to surface contention hotspots.

Concurrency is a cross‑cutting concern; new modules should expose their
read/write requirements explicitly so they can integrate with these
primitives rather than reinventing bespoke locks.

## 5. Reentrant proof-of-service classes

The runtime now routes gossip, compute, and storage tasks through a reentrant
scheduler that enforces weighted fairness. Each class receives a configurable
weight (governed via `scheduler_weight_{gossip,compute,storage}` parameters)
representing the number of consecutive tasks it may execute before yielding.
When the optional `reentrant_scheduler` feature flag is disabled the scheduler
falls back to a simple FIFO queue, ensuring compatibility for lightweight
builds.

Every queued task records its enqueue instant. When the scheduler dispatches the
workload it emits `scheduler_class_wait_seconds{class="..."}` so operators can
verify latency budgets. The CLI exposes the aggregated view via
`blockctl scheduler stats`, and the RPC endpoint `scheduler.stats` mirrors the
same payload for dashboards.

For deterministic execution, callers may convert the drained tasks directly into
`ParallelExecutor` groups via `ServiceScheduler::execute_ready`, enabling
parallel execution without sacrificing fairness between classes.
